/// Slide extraction pipeline for PDF and PPTX lecture files.
///
/// # Pipeline
///
/// 1. **Extraction** — per-format backends produce a `Vec<RawSlide>`.
/// 2. **Boilerplate stripping** — lines appearing on ≥ 60 % of slides are
///    identified as deck-wide boilerplate (copyright notices, institution
///    names, running headers/footers) and removed from every slide.
/// 3. **Fuzzy dedup** — slides whose post-strip word-set Jaccard similarity
///    to any of the preceding 5 non-skipped slides exceeds 0.85 are dropped.
///    This collapses "same code with different parts highlighted" sequences.
///    Sparse slides (under `VISUAL_MAX_WORDS` words once boilerplate is
///    removed) are exempt: they are usually diagrams, plots or screenshots,
///    so they are kept and flagged `visual` instead.
/// 4. **Images** — optionally, visual slides get an image the model can look
///    at: PDF pages are rendered with `pdftoppm`, PPTX slides contribute
///    their embedded pictures.  Images land in the workspace's `_images/`.
/// 5. **Formatting** — remaining slides are rendered to Markdown and returned
///    as a single string ready for XML-wrapping into the workspace.
///
/// Extraction failures (tool not found, corrupt archive) return a `SlideError`
/// whose message is shown to the user as a soft warning; the session continues
/// without the artifact.
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SlideError(pub String);

impl std::fmt::Display for SlideError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SlideError {}

// anyhow's blanket `impl<E: StdError + Send + Sync + 'static> From<E>` already
// covers SlideError — no manual From impl needed.

type Result<T> = std::result::Result<T, SlideError>;

// ── Data types ────────────────────────────────────────────────────────────────

/// A single slide as extracted from the source file — no processing applied.
#[derive(Debug, Clone)]
pub struct RawSlide {
    /// 1-based slide number.
    pub number: usize,
    /// Text lines extracted from the slide body.
    pub lines: Vec<String>,
    /// Speaker notes, if present.
    pub notes: Option<String>,
}

/// A slide after boilerplate stripping and dedup.
#[derive(Debug, Clone)]
pub struct Slide {
    pub number: usize,
    pub lines: Vec<String>,
    pub notes: Option<String>,
    /// Too little text to stand on its own — probably a diagram, plot or
    /// screenshot.  Marked in the Markdown so the model knows to look.
    pub visual: bool,
    /// Workspace-relative paths of images attached to this slide.
    pub images: Vec<String>,
}

// ── Tunables ──────────────────────────────────────────────────────────────────

/// Fraction of slides a normalised line must appear in to be treated as
/// deck-wide boilerplate and stripped before dedup/formatting.
const BOILERPLATE_THRESHOLD: f64 = 0.60;

/// Word-set Jaccard similarity above which a slide is considered a duplicate
/// of a recently-seen slide and dropped.
const DEDUP_SIMILARITY: f64 = 0.85;

/// How many preceding non-skipped slides to compare against for dedup.
const DEDUP_WINDOW: usize = 5;

/// A slide with fewer words than this (after boilerplate stripping) is
/// treated as visual: kept, exempt from dedup, and eligible for an image.
const VISUAL_MAX_WORDS: usize = 8;

/// Workspace subdirectory that holds slide and artifact images.
pub const IMAGES_DIR: &str = "_images";

/// Most images copied into one workspace.  Keeps a long image-heavy deck
/// from flooding the model's context.
pub const MAX_IMAGES_PER_WORKSPACE: usize = 30;

/// Resolution for rendered PDF pages: legible slide text without huge files.
const RENDER_DPI: &str = "110";

/// Raster formats a model can view.  Vector formats embedded in PPTX
/// (EMF/WMF) are skipped.
pub const VIEWABLE_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

// ── Public entry points ───────────────────────────────────────────────────────

/// Extract slides from a PDF file using the `pdftotext` CLI tool.
///
/// Requires `pdftotext` (from poppler-utils) to be on `$PATH`.
/// Returns `Err(SlideError)` if the tool is not found or extraction fails.
pub fn extract_pdf(path: &Path) -> Result<Vec<RawSlide>> {
    let output = std::process::Command::new("pdftotext")
        .arg(path) // PDF-file
        .arg("-") // text-file: "-" means stdout
        .output();

    // pdftotext syntax: pdftotext [options] <PDF-file> [<text-file>]
    // Use "-" as the output file to get stdout.
    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SlideError(format!(
                "pdftotext not found — install poppler-utils to extract PDF slides \
                 ({})",
                path.display()
            )));
        }
        Err(e) => {
            return Err(SlideError(format!(
                "failed to run pdftotext on '{}': {}",
                path.display(),
                e
            )));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SlideError(format!(
            "pdftotext failed on '{}': {}",
            path.display(),
            stderr.trim()
        )));
    }

    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(pages_to_raw_slides(text))
}

/// Extract text from a DOCX file using the `pandoc` CLI tool.
///
/// Requires `pandoc` to be on `$PATH`.  Returns the extracted content as a
/// Markdown string ready for XML-wrapping into the workspace.  Unlike the
/// slide pipeline, DOCX output is returned directly — document structure
/// (headings, lists, tables) is preserved as Markdown rather than being
/// converted to the slide `RawSlide` model.
///
/// Returns `Err(SlideError)` if `pandoc` is not found or extraction fails.
pub fn extract_docx(path: &Path) -> Result<String> {
    let output = std::process::Command::new("pandoc")
        .arg("--from=docx")
        .arg("--to=markdown")
        .arg("--wrap=none")
        .arg(path)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(SlideError(format!(
                "pandoc not found — install pandoc to extract DOCX sources \
                 ({})",
                path.display()
            )));
        }
        Err(e) => {
            return Err(SlideError(format!(
                "failed to run pandoc on '{}': {}",
                path.display(),
                e
            )));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(SlideError(format!(
            "pandoc failed on '{}': {}",
            path.display(),
            stderr.trim()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Extract slides from a PPTX file (pure Rust — no external tools required).
///
/// Returns `Err(SlideError)` if the file is not a valid PPTX archive.
pub fn extract_pptx(path: &Path) -> Result<Vec<RawSlide>> {
    let file = std::fs::File::open(path)
        .map_err(|e| SlideError(format!("cannot open '{}': {}", path.display(), e)))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        SlideError(format!(
            "'{}' is not a valid PPTX file: {}",
            path.display(),
            e
        ))
    })?;

    // Collect slide indices so we can process them in order.
    let slide_count = (1..)
        .take_while(|n| {
            archive
                .by_name(&format!("ppt/slides/slide{}.xml", n))
                .is_ok()
        })
        .count();

    let mut raw_slides = Vec::with_capacity(slide_count);

    for n in 1..=slide_count {
        let lines = read_pptx_slide_text(&mut archive, n)?;
        let notes = read_pptx_notes_text(&mut archive, n);
        raw_slides.push(RawSlide {
            number: n,
            lines,
            notes,
        });
    }

    Ok(raw_slides)
}

/// Run the full fuzzy-dedup + boilerplate-strip pipeline on extracted slides.
///
/// **Order matters**: dedup runs first so that repeated footers count toward
/// slide similarity (helping collapse "same code, different highlight" runs),
/// and boilerplate frequency is then computed on the *deduplicated* set.  If
/// boilerplate were stripped first, a slide where every line happens to repeat
/// across all slides (e.g. three identical code slides shown for emphasis)
/// would have all its content removed before dedup ever fires.
///
/// Sparse slides are never dropped.  Sparseness is judged against the
/// boilerplate found by a first, plain dedup pass (the original pipeline), so
/// a slide holding only a title and the deck footer counts as sparse; the
/// second pass then dedups with sparse slides exempt.
pub fn process(slides: Vec<RawSlide>) -> Vec<Slide> {
    if slides.is_empty() {
        return Vec::new();
    }

    // Phase 1: fuzzy dedup with a sliding window, run once without
    // exemptions purely to find the deck's boilerplate.
    // Repeated footer lines increase pairwise similarity, making it easier to
    // collapse near-identical slides (same code, varying highlights).
    // Boilerplate frequency is computed over unique slides only, so genuinely
    // repeated content (code shown 3× for emphasis, deduplicated to 1 slide)
    // cannot be mistakenly flagged as per-deck boilerplate.
    let boilerplate = find_boilerplate(&dedup(slides.clone(), |_| false));

    // Phase 2: the real dedup, with sparse (likely visual) slides exempt.
    let is_sparse = |s: &RawSlide| content_word_count(&s.lines, &boilerplate) < VISUAL_MAX_WORDS;
    let deduped = dedup(slides, is_sparse);

    // Phase 3: strip boilerplate.
    deduped
        .into_iter()
        .map(|s| {
            let lines: Vec<String> = s
                .lines
                .into_iter()
                .filter(|l| !boilerplate.contains(&normalise_line(l)))
                .filter(|l| !l.trim().is_empty())
                .collect();
            let visual = content_word_count(&lines, &HashSet::new()) < VISUAL_MAX_WORDS;
            Slide {
                number: s.number,
                lines,
                notes: s.notes,
                visual,
                images: Vec::new(),
            }
        })
        .collect()
}

/// Drop slides whose word set is a near-duplicate of one of the previous
/// `DEDUP_WINDOW` kept slides.  `exempt` slides are always kept; other
/// slides with no words are dropped.
fn dedup(slides: Vec<RawSlide>, exempt: impl Fn(&RawSlide) -> bool) -> Vec<RawSlide> {
    let mut kept: Vec<RawSlide> = Vec::with_capacity(slides.len());
    for slide in slides {
        if exempt(&slide) {
            kept.push(slide);
            continue;
        }
        let sig = word_set_signature(&slide.lines);
        if sig.is_empty() {
            continue;
        }
        let is_dup = kept
            .iter()
            .rev()
            .take(DEDUP_WINDOW)
            .any(|prev| jaccard(&sig, &word_set_signature(&prev.lines)) >= DEDUP_SIMILARITY);
        if !is_dup {
            kept.push(slide);
        }
    }
    kept
}

/// Render each visual slide's PDF page to a PNG in `<workspace>/_images/`
/// and record the path on the slide.  Stops at `budget` images.
///
/// Soft-fails: a missing `pdftoppm` warns once per process and leaves the
/// slides with their text markers; a page that fails to render is skipped.
pub fn render_pdf_visuals(
    pdf: &Path,
    slides: &mut [Slide],
    workspace_root: &Path,
    stem: &str,
    budget: &mut usize,
) {
    render_pdf_visuals_with("pdftoppm", pdf, slides, workspace_root, stem, budget);
}

/// `render_pdf_visuals` with the renderer binary named explicitly, so tests
/// can exercise the missing-tool path.
fn render_pdf_visuals_with(
    tool: &str,
    pdf: &Path,
    slides: &mut [Slide],
    workspace_root: &Path,
    stem: &str,
    budget: &mut usize,
) {
    static PDFTOPPM_MISSING_WARNED: AtomicBool = AtomicBool::new(false);

    let out_dir = workspace_root.join(IMAGES_DIR);
    let mut skipped = 0;
    for slide in slides.iter_mut().filter(|s| s.visual) {
        if *budget == 0 {
            skipped += 1;
            continue;
        }
        if std::fs::create_dir_all(&out_dir).is_err() {
            return;
        }
        let name = format!("{}-p{}", stem, slide.number);
        let page = slide.number.to_string();
        // -singlefile writes exactly `<prefix>.png` with no page suffix.
        let status = std::process::Command::new(tool)
            .args([
                "-png",
                "-r",
                RENDER_DPI,
                "-singlefile",
                "-f",
                &page,
                "-l",
                &page,
            ])
            .arg(pdf)
            .arg(out_dir.join(&name))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        match status {
            Ok(st) if st.success() => {
                slide.images.push(format!("{}/{}.png", IMAGES_DIR, name));
                *budget -= 1;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if !PDFTOPPM_MISSING_WARNED.swap(true, Ordering::Relaxed) {
                    crate::ui::warn_line(
                        "warning: pdftoppm not found — visual slides are marked but not \
                         rendered (install poppler to include slide images)",
                    );
                }
                return;
            }
            Err(_) => {}
        }
    }
    warn_budget_exhausted(pdf, skipped);
}

/// Copy the pictures embedded in each visual PPTX slide into
/// `<workspace>/_images/` and record their paths.  Stops at `budget` images.
/// Vector formats and external links are skipped.
///
/// Unlike a PDF page, a PPTX slide says what it contains: a sparse slide with
/// no pictures (a title or section divider) is un-flagged rather than marked
/// as a visual with no image.
pub fn extract_pptx_visuals(
    pptx: &Path,
    slides: &mut [Slide],
    workspace_root: &Path,
    stem: &str,
    budget: &mut usize,
) {
    let Ok(file) = std::fs::File::open(pptx) else {
        return;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return;
    };
    let out_dir = workspace_root.join(IMAGES_DIR);
    let mut skipped = 0;
    for slide in slides.iter_mut().filter(|s| s.visual) {
        let targets = read_pptx_image_targets(&mut archive, slide.number);
        if targets.is_empty() {
            slide.visual = false;
            continue;
        }
        for (k, target) in targets.iter().enumerate() {
            if *budget == 0 {
                skipped += 1;
                break;
            }
            let ext = target
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            let mut bytes = Vec::new();
            let read_ok = archive
                .by_name(target)
                .map(|mut e| e.read_to_end(&mut bytes).is_ok())
                .unwrap_or(false);
            if !read_ok || std::fs::create_dir_all(&out_dir).is_err() {
                continue;
            }
            let name = format!("{}-s{}-{}.{}", stem, slide.number, k + 1, ext);
            if std::fs::write(out_dir.join(&name), &bytes).is_ok() {
                slide.images.push(format!("{}/{}", IMAGES_DIR, name));
                *budget -= 1;
            }
        }
    }
    warn_budget_exhausted(pptx, skipped);
}

fn warn_budget_exhausted(path: &Path, skipped: usize) {
    if skipped > 0 {
        crate::ui::warn_line(format!(
            "warning: image cap ({}) reached — {} visual slide(s) in '{}' left as text markers",
            MAX_IMAGES_PER_WORKSPACE,
            skipped,
            path.display()
        ));
    }
}

/// Format processed slides as a Markdown string for workspace ingestion.
pub fn to_workspace_markdown(slides: &[Slide]) -> String {
    if slides.is_empty() {
        return "_No readable slide content extracted._\n".to_string();
    }
    let mut out = String::new();
    for slide in slides {
        out.push_str(&format!("### Slide {}\n\n", slide.number));
        for line in &slide.lines {
            out.push_str(line);
            out.push('\n');
        }
        if slide.visual {
            if !slide.lines.is_empty() {
                out.push('\n');
            }
            if slide.images.is_empty() {
                out.push_str("_[Mostly visual — no image available]_\n");
            } else {
                let paths: Vec<String> = slide.images.iter().map(|p| format!("`{}`", p)).collect();
                out.push_str(&format!("_[Mostly visual — view {}]_\n", paths.join(", ")));
            }
        }
        if let Some(notes) = &slide.notes {
            let trimmed = notes.trim();
            if !trimmed.is_empty() {
                out.push_str(&format!("\n_Notes: {}_\n", trimmed));
            }
        }
        out.push('\n');
    }
    out
}

// ── PDF helpers ───────────────────────────────────────────────────────────────

/// Split `pdftotext` output (page-separated by form-feed `\x0c`) into
/// per-slide `RawSlide` values.  Pages without text are kept (with no lines)
/// so image-only slides survive to be marked visual; only the empty piece
/// after the final form-feed is dropped.
fn pages_to_raw_slides(text: String) -> Vec<RawSlide> {
    let mut pages: Vec<&str> = text.split('\x0c').collect();
    if pages.last().is_some_and(|p| p.trim().is_empty()) {
        pages.pop();
    }
    pages
        .into_iter()
        .enumerate()
        .map(|(i, page)| RawSlide {
            number: i + 1,
            lines: page
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            notes: None,
        })
        .collect()
}

// ── PPTX helpers ─────────────────────────────────────────────────────────────

/// Extract all text from a single PPTX slide XML entry.
fn read_pptx_slide_text(
    archive: &mut zip::ZipArchive<std::fs::File>,
    n: usize,
) -> Result<Vec<String>> {
    let name = format!("ppt/slides/slide{}.xml", n);
    let mut entry = archive
        .by_name(&name)
        .map_err(|e| SlideError(format!("cannot read slide {} from PPTX: {}", n, e)))?;
    let mut xml = String::new();
    entry
        .read_to_string(&mut xml)
        .map_err(|e| SlideError(format!("cannot read slide {} XML: {}", n, e)))?;
    Ok(extract_xml_text_nodes(&xml))
}

/// Extract speaker notes from a PPTX notes slide XML entry (soft — returns
/// `None` if the entry doesn't exist or can't be read).
fn read_pptx_notes_text(archive: &mut zip::ZipArchive<std::fs::File>, n: usize) -> Option<String> {
    let name = format!("ppt/notesSlides/notesSlide{}.xml", n);
    let mut entry = archive.by_name(&name).ok()?;
    let mut xml = String::new();
    entry.read_to_string(&mut xml).ok()?;
    let lines = extract_xml_text_nodes(&xml);
    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" "))
    }
}

/// List the zip paths of viewable images a PPTX slide references, in
/// relationship order.  Soft — returns an empty list on any read failure.
fn read_pptx_image_targets(archive: &mut zip::ZipArchive<std::fs::File>, n: usize) -> Vec<String> {
    let name = format!("ppt/slides/_rels/slide{}.xml.rels", n);
    let mut xml = String::new();
    match archive.by_name(&name) {
        Ok(mut entry) => {
            if entry.read_to_string(&mut xml).is_err() {
                return Vec::new();
            }
        }
        Err(_) => return Vec::new(),
    }
    let mut targets: Vec<String> = Vec::new();
    for target in parse_image_relationships(&xml) {
        let path = resolve_pptx_target(&target);
        let ext = path
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if VIEWABLE_IMAGE_EXTENSIONS.contains(&ext.as_str()) && !targets.contains(&path) {
            targets.push(path);
        }
    }
    targets
}

/// Pull the `Target` of every internal image `<Relationship>` from a
/// slide's `.rels` XML.
fn parse_image_relationships(xml: &str) -> Vec<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if e.local_name().as_ref() == "Relationship" =>
            {
                let mut is_image = false;
                let mut external = false;
                let mut target = None;
                for attr in e.attributes().flatten() {
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .unwrap_or_default()
                        .into_owned();
                    match attr.key.as_ref() {
                        "Type" => is_image = value.ends_with("/image"),
                        "TargetMode" => external = value == "External",
                        "Target" => target = Some(value),
                        _ => {}
                    }
                }
                if let (true, false, Some(t)) = (is_image, external, target) {
                    out.push(t);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Resolve a relationship target (relative to `ppt/slides/`) to a zip path.
fn resolve_pptx_target(target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = vec!["ppt", "slides"];
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Extract the text content of all `<a:t>` elements from a PowerPoint XML
/// string using `quick-xml`.  Preserves run order within each paragraph but
/// joins runs with no separator (PowerPoint splits runs on formatting
/// boundaries, not word boundaries).
fn extract_xml_text_nodes(xml: &str) -> Vec<String> {
    use quick_xml::escape::resolve_predefined_entity;
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    // Do NOT trim_text globally — it would eat the space between concatenated
    // runs (e.g. "Second " + "line" → "Secondline").  We trim only at the
    // paragraph-emit step instead.

    let mut lines: Vec<String> = Vec::new();
    let mut current_para: Option<String> = None;
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => match e.name().as_ref() {
                "a:p" => {
                    current_para = Some(String::new());
                }
                "a:t" => {
                    in_text = true;
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                "a:p" => {
                    if let Some(para) = current_para.take() {
                        let trimmed = para.trim().to_string();
                        if !trimmed.is_empty() {
                            lines.push(trimmed);
                        }
                    }
                }
                "a:t" => {
                    in_text = false;
                }
                _ => {}
            },
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Some(ref mut para) = current_para {
                        para.push_str(&e.xml10_content());
                    }
                }
            }
            // Entity and character references (`&amp;`, `&#x2013;`) arrive as
            // separate events between the surrounding `Text` events.
            Ok(Event::GeneralRef(e)) => {
                if in_text {
                    if let Some(ref mut para) = current_para {
                        if let Ok(Some(ch)) = e.resolve_char_ref() {
                            para.push(ch);
                        } else if let Some(s) = resolve_predefined_entity(&e) {
                            para.push_str(s);
                        } else {
                            // Unknown entity: keep it verbatim rather than drop text.
                            para.push('&');
                            para.push_str(&e);
                            para.push(';');
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    lines
}

// ── Processing helpers ────────────────────────────────────────────────────────

fn normalise_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Find the set of normalised lines that appear on ≥ BOILERPLATE_THRESHOLD
/// fraction of the slide deck AND on at least 2 slides in absolute terms.
/// These are stripped before formatting.
///
/// The absolute-minimum-2 guard prevents mistaking slide content for
/// boilerplate when only one unique slide survives deduplication (e.g. three
/// identical code slides collapse to one — after dedup that one slide has
/// 100 % "frequency" for all its lines, which would incorrectly flag the
/// actual content as boilerplate).
fn find_boilerplate(slides: &[RawSlide]) -> HashSet<String> {
    let total = slides.len();
    if total < 2 {
        // Cannot distinguish boilerplate from content with fewer than 2 slides.
        return HashSet::new();
    }
    let mut freq: HashMap<String, usize> = HashMap::new();
    for slide in slides {
        // Count each distinct normalised line once per slide.
        let seen: HashSet<String> = slide.lines.iter().map(|l| normalise_line(l)).collect();
        for line in seen {
            if !line.is_empty() {
                *freq.entry(line).or_insert(0) += 1;
            }
        }
    }
    let threshold_count = (total as f64 * BOILERPLATE_THRESHOLD).ceil() as usize;
    freq.into_iter()
        .filter(|(_, count)| *count >= threshold_count)
        .map(|(line, _)| line)
        .collect()
}

/// Count the words on a slide, ignoring lines in `boilerplate`.
fn content_word_count(lines: &[String], boilerplate: &HashSet<String>) -> usize {
    lines
        .iter()
        .filter(|l| !boilerplate.contains(&normalise_line(l)))
        .map(|l| l.split_whitespace().count())
        .sum()
}

/// Build the word multiset signature for a slide's lines (used for Jaccard).
fn word_set_signature(lines: &[String]) -> HashSet<String> {
    lines
        .iter()
        .flat_map(|l| l.split_whitespace())
        .map(|w| w.to_lowercase())
        .collect()
}

/// Jaccard similarity between two word sets: |A ∩ B| / |A ∪ B|.
fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 1.0;
    }
    intersection as f64 / union as f64
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn raw(n: usize, lines: &[&str]) -> RawSlide {
        RawSlide {
            number: n,
            lines: lines.iter().map(|s| s.to_string()).collect(),
            notes: None,
        }
    }

    fn raw_with_notes(n: usize, lines: &[&str], notes: &str) -> RawSlide {
        RawSlide {
            number: n,
            lines: lines.iter().map(|s| s.to_string()).collect(),
            notes: Some(notes.to_string()),
        }
    }

    // ── normalise_line ────────────────────────────────────────────────────────

    #[test]
    fn normalise_collapses_whitespace_and_lowercases() {
        assert_eq!(normalise_line("  Hello   World  "), "hello world");
        assert_eq!(normalise_line("CPSC 5001 © 2024"), "cpsc 5001 © 2024");
    }

    // ── find_boilerplate ──────────────────────────────────────────────────────

    #[test]
    fn boilerplate_detected_on_most_slides() {
        // Copyright line appears on all 4 slides — should be flagged.
        // "unique content" appears only once — should not be flagged.
        let slides = vec![
            raw(1, &["© 2024 University", "Sorting algorithms"]),
            raw(2, &["© 2024 University", "Merge sort"]),
            raw(3, &["© 2024 University", "Quick sort"]),
            raw(4, &["© 2024 University", "Heap sort"]),
        ];
        let bp = find_boilerplate(&slides);
        assert!(
            bp.contains("© 2024 university"),
            "copyright must be boilerplate"
        );
        assert!(
            !bp.contains("sorting algorithms"),
            "unique line must not be flagged"
        );
    }

    #[test]
    fn boilerplate_not_flagged_when_appears_on_minority_of_slides() {
        // Appears on 2 of 5 slides (40 %) — below 60 % threshold.
        let slides = vec![
            raw(1, &["repeated", "unique-a"]),
            raw(2, &["repeated", "unique-b"]),
            raw(3, &["unique-c"]),
            raw(4, &["unique-d"]),
            raw(5, &["unique-e"]),
        ];
        let bp = find_boilerplate(&slides);
        assert!(
            !bp.contains("repeated"),
            "40 % frequency must not be flagged"
        );
    }

    // ── jaccard ───────────────────────────────────────────────────────────────

    #[test]
    fn jaccard_identical_sets_returns_one() {
        let a: HashSet<String> = ["foo", "bar", "baz"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let b = a.clone();
        assert!((jaccard(&a, &b) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn jaccard_disjoint_sets_returns_zero() {
        let a: HashSet<String> = ["foo", "bar"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["baz", "qux"].iter().map(|s| s.to_string()).collect();
        assert!((jaccard(&a, &b)).abs() < 1e-9);
    }

    #[test]
    fn jaccard_partial_overlap() {
        // {a, b, c} ∩ {b, c, d} = {b, c}  →  2/4 = 0.5
        let a: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert!((jaccard(&a, &b) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn jaccard_both_empty_returns_one() {
        let empty: HashSet<String> = HashSet::new();
        assert!((jaccard(&empty, &empty) - 1.0).abs() < 1e-9);
    }

    // ── process: dedup ────────────────────────────────────────────────────────

    #[test]
    fn process_drops_exact_duplicate_slides() {
        // Slides 2 and 3 are identical to slide 1 — should be dropped.
        let content = &["int x = 5;", "int y = x + 1;", "return y;"];
        let slides = vec![raw(1, content), raw(2, content), raw(3, content)];
        let result = process(slides);
        assert_eq!(result.len(), 1, "only one unique slide should survive");
        assert_eq!(result[0].number, 1);
    }

    #[test]
    fn process_keeps_sufficiently_different_slides() {
        // Jaccard between these should be well below 0.85.
        let slides = vec![
            raw(
                1,
                &[
                    "Sorting algorithms overview",
                    "comparison-based",
                    "O(n log n) lower bound",
                ],
            ),
            raw(
                2,
                &[
                    "Hash tables",
                    "amortized O(1) insert",
                    "load factor",
                    "collision resolution",
                ],
            ),
        ];
        let result = process(slides);
        assert_eq!(result.len(), 2, "distinct slides must both be kept");
    }

    #[test]
    fn process_drops_near_duplicate_code_slides() {
        // Same code snippet repeated with one highlighted word changed — typical
        // "build-up" slide pattern.  After word-set Jaccard both should score
        // well above 0.85 since the vocabulary is nearly identical.
        let base = vec![
            "public static int binarySearch(int[] arr, int target) {",
            "    int lo = 0, hi = arr.length - 1;",
            "    while (lo <= hi) {",
            "        int mid = lo + (hi - lo) / 2;",
            "        if (arr[mid] == target) return mid;",
            "        if (arr[mid] < target) lo = mid + 1;",
            "        else hi = mid - 1;",
            "    }",
            "    return -1;",
            "}",
        ];
        // Second slide: adds exactly one extra comment line.
        let mut variant = base.clone();
        variant.push("// highlight: lo = mid + 1");
        let slides = vec![raw(1, &base), raw(2, &variant)];
        let result = process(slides);
        assert_eq!(
            result.len(),
            1,
            "near-duplicate code slides should be deduped; got: {:?}",
            result.iter().map(|s| s.number).collect::<Vec<_>>()
        );
    }

    #[test]
    fn process_strips_boilerplate_before_comparing() {
        // Without stripping, copyright-only slides would look different from
        // each other because they differ in title.  After stripping the
        // boilerplate footer, only the unique title remains.
        //
        // Here slides 1 and 2 differ only in their unique content so they
        // must both survive; the copyright line must disappear.
        let slides = vec![
            raw(1, &["Slide title A", "© 2024 University CS Dept"]),
            raw(2, &["Slide title B", "© 2024 University CS Dept"]),
            raw(3, &["Slide title C", "© 2024 University CS Dept"]),
            raw(4, &["Slide title D", "© 2024 University CS Dept"]),
        ];
        let result = process(slides);
        // All four have unique titles → all four should survive.
        assert_eq!(
            result.len(),
            4,
            "all unique slides must survive after stripping"
        );
        // Copyright line must not appear in output.
        for slide in &result {
            for line in &slide.lines {
                assert!(
                    !line.to_lowercase().contains("© 2024"),
                    "boilerplate must be stripped: {:?}",
                    line
                );
            }
        }
    }

    #[test]
    fn process_empty_slide_after_strip_is_kept_as_visual() {
        // A slide consisting entirely of boilerplate lines is usually an
        // image-only slide: it must survive, flagged visual, with no text.
        let slides = vec![
            raw(
                1,
                &[
                    "Real content here about raw pointers and references",
                    "© Footer",
                ],
            ),
            raw(2, &["© Footer"]), // becomes empty after stripping
            raw(
                3,
                &[
                    "More content on smart pointers and ownership rules",
                    "© Footer",
                ],
            ),
            raw(
                4,
                &[
                    "Even more content covering move semantics in depth",
                    "© Footer",
                ],
            ),
        ];
        let result = process(slides);
        assert_eq!(result.len(), 4);
        let s2 = result.iter().find(|s| s.number == 2).unwrap();
        assert!(s2.visual, "boilerplate-only slide must be marked visual");
        assert!(s2.lines.is_empty());
        assert!(
            result.iter().filter(|s| s.number != 2).all(|s| !s.visual),
            "text-heavy slides must not be marked visual"
        );
    }

    #[test]
    fn process_keeps_blank_pdf_page() {
        let slides = vec![
            raw(
                1,
                &["Templates let one function work across many types at once"],
            ),
            raw(2, &[]),
        ];
        let result = process(slides);
        assert_eq!(result.len(), 2);
        assert!(result[1].visual);
    }

    #[test]
    fn process_does_not_dedup_sparse_slides() {
        // Three "Example" title slides (each followed by a diagram) look
        // identical as text but show different pictures.
        let slides = vec![
            raw(1, &["Example", "CPSC 5005 Fall"]),
            raw(2, &["Example", "CPSC 5005 Fall"]),
            raw(3, &["Example", "CPSC 5005 Fall"]),
        ];
        let result = process(slides);
        assert_eq!(result.len(), 3, "sparse slides are exempt from dedup");
        assert!(result.iter().all(|s| s.visual));
    }

    #[test]
    fn process_title_plus_footer_counts_as_sparse_before_dedup() {
        // Title + an 8-word footer clears VISUAL_MAX_WORDS before stripping,
        // but the provisional boilerplate pass sees through the footer.
        let footer = "Copyright 2026 Seattle University all rights reserved here";
        let slides = vec![
            raw(1, &["Heap layout diagram", footer]),
            raw(2, &["Heap layout diagram", footer]),
            raw(
                3,
                &[
                    "Stack frames hold locals, parameters and the return address",
                    footer,
                ],
            ),
        ];
        let result = process(slides);
        assert_eq!(
            result.iter().filter(|s| s.number <= 2).count(),
            2,
            "both title-only slides must survive"
        );
        assert!(result.iter().filter(|s| s.number <= 2).all(|s| s.visual));
    }

    #[test]
    fn process_still_dedups_text_heavy_slides() {
        let body = &[
            "int main() { std::vector<int> v{1, 2, 3};",
            "for (auto x : v) std::cout << x << std::endl;",
            "return 0; }",
        ];
        let slides = vec![raw(1, body), raw(2, body)];
        let result = process(slides);
        assert_eq!(result.len(), 1);
        assert!(!result[0].visual);
    }

    #[test]
    fn process_preserves_speaker_notes() {
        let slides = vec![
            raw_with_notes(1, &["Topic: arrays"], "Mention cache locality here."),
            raw_with_notes(2, &["Topic: linked lists"], "Draw the pointer diagram."),
        ];
        let result = process(slides);
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].notes.as_deref(),
            Some("Mention cache locality here.")
        );
        assert_eq!(
            result[1].notes.as_deref(),
            Some("Draw the pointer diagram.")
        );
    }

    // ── pages_to_raw_slides ───────────────────────────────────────────────────

    #[test]
    fn pages_to_raw_slides_splits_on_form_feed() {
        let text = "Page one line A\nPage one line B\x0cPage two\x0c".to_string();
        let slides = pages_to_raw_slides(text);
        assert_eq!(
            slides.len(),
            2,
            "trailing form-feed must not produce empty slide"
        );
        assert_eq!(slides[0].number, 1);
        assert_eq!(slides[0].lines, vec!["Page one line A", "Page one line B"]);
        assert_eq!(slides[1].number, 2);
        assert_eq!(slides[1].lines, vec!["Page two"]);
    }

    #[test]
    fn pages_to_raw_slides_trims_whitespace() {
        let text = "  leading space  \n  indented  \x0c".to_string();
        let slides = pages_to_raw_slides(text);
        assert_eq!(slides[0].lines, vec!["leading space", "indented"]);
    }

    // ── extract_xml_text_nodes ────────────────────────────────────────────────

    #[test]
    fn extract_xml_extracts_at_text_nodes() {
        let xml = r#"<?xml version="1.0"?>
<p:sld>
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:txBody>
          <a:p><a:r><a:t>Hello world</a:t></a:r></a:p>
          <a:p><a:r><a:t>Second </a:t></a:r><a:r><a:t>line</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#;
        let lines = extract_xml_text_nodes(xml);
        assert_eq!(lines, vec!["Hello world", "Second line"]);
    }

    #[test]
    fn extract_xml_skips_empty_paragraphs() {
        let xml = r#"<root><a:p><a:r><a:t>  </a:t></a:r></a:p><a:p><a:r><a:t>Real</a:t></a:r></a:p></root>"#;
        let lines = extract_xml_text_nodes(xml);
        assert_eq!(lines, vec!["Real"]);
    }

    #[test]
    fn extract_xml_unescapes_entities() {
        let xml = r#"<root><a:p><a:r><a:t>Q&amp;A: x &lt; y &#x2013; &#65;&bogus;</a:t></a:r></a:p></root>"#;
        let lines = extract_xml_text_nodes(xml);
        assert_eq!(lines, vec!["Q&A: x < y \u{2013} A&bogus;"]);
    }

    // ── to_workspace_markdown ─────────────────────────────────────────────────

    #[test]
    fn to_workspace_markdown_formats_slide_with_notes() {
        let slides = vec![Slide {
            number: 3,
            lines: vec!["Quicksort".to_string(), "O(n log n) average".to_string()],
            notes: Some("Mention worst-case O(n²) on sorted input.".to_string()),
            visual: false,
            images: vec![],
        }];
        let md = to_workspace_markdown(&slides);
        assert!(md.contains("### Slide 3"));
        assert!(md.contains("Quicksort"));
        assert!(md.contains("O(n log n) average"));
        assert!(md.contains("_Notes: Mention worst-case O(n²) on sorted input._"));
    }

    #[test]
    fn to_workspace_markdown_empty_notes_not_printed() {
        let slides = vec![Slide {
            number: 1,
            lines: vec!["Title".to_string()],
            notes: Some("   ".to_string()), // whitespace-only notes
            visual: false,
            images: vec![],
        }];
        let md = to_workspace_markdown(&slides);
        assert!(
            !md.contains("_Notes:"),
            "empty notes must not appear in output"
        );
    }

    #[test]
    fn to_workspace_markdown_empty_slides_returns_placeholder() {
        let md = to_workspace_markdown(&[]);
        assert!(md.contains("No readable slide content"));
    }

    #[test]
    fn pages_to_raw_slides_keeps_blank_middle_page() {
        let text = "Intro\x0c   \n\x0cOutro\x0c".to_string();
        let slides = pages_to_raw_slides(text);
        assert_eq!(slides.len(), 3);
        assert_eq!(slides[1].number, 2);
        assert!(slides[1].lines.is_empty());
        assert_eq!(slides[2].number, 3);
    }

    fn visual_slide(number: usize, lines: &[&str], images: &[&str]) -> Slide {
        Slide {
            number,
            lines: lines.iter().map(|s| s.to_string()).collect(),
            notes: None,
            visual: true,
            images: images.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn to_workspace_markdown_marks_visual_slide_with_image() {
        let md = to_workspace_markdown(&[visual_slide(4, &["Heap"], &["_images/deck-p4.png"])]);
        assert!(
            md.contains("### Slide 4\n\nHeap\n\n_[Mostly visual — view `_images/deck-p4.png`]_")
        );
    }

    #[test]
    fn to_workspace_markdown_marks_visual_slide_without_image() {
        let md = to_workspace_markdown(&[visual_slide(2, &[], &[])]);
        assert!(md.contains("### Slide 2\n\n_[Mostly visual — no image available]_"));
    }

    #[test]
    fn parse_image_relationships_keeps_internal_images_only() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout2.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.com/x.png" TargetMode="External"/>
</Relationships>"#;
        assert_eq!(parse_image_relationships(xml), vec!["../media/image1.png"]);
    }

    #[test]
    fn resolve_pptx_target_handles_relative_and_absolute() {
        assert_eq!(
            resolve_pptx_target("../media/image1.png"),
            "ppt/media/image1.png"
        );
        assert_eq!(
            resolve_pptx_target("/ppt/media/image2.jpeg"),
            "ppt/media/image2.jpeg"
        );
        assert_eq!(resolve_pptx_target("media/x.png"), "ppt/slides/media/x.png");
    }

    /// Build a three-slide PPTX: slide 1 is text-heavy, slide 2 has only a
    /// title plus one PNG and one EMF picture, slide 3 is a bare title.
    fn write_pptx_fixture(path: &Path) {
        use std::io::Write;
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        let slide = |text: &str| {
            format!(
                r#"<p:sld><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#,
                text
            )
        };
        let img = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
        let entries: Vec<(String, Vec<u8>)> = vec![
            (
                "ppt/slides/slide1.xml".into(),
                slide("Virtual functions dispatch through the vtable at runtime always").into_bytes(),
            ),
            ("ppt/slides/slide2.xml".into(), slide("Class diagram").into_bytes()),
            ("ppt/slides/slide3.xml".into(), slide("Part two").into_bytes()),
            (
                "ppt/slides/_rels/slide2.xml.rels".into(),
                format!(
                    r#"<Relationships><Relationship Id="rId2" Type="{img}" Target="../media/image1.png"/><Relationship Id="rId3" Type="{img}" Target="../media/image2.emf"/></Relationships>"#
                )
                .into_bytes(),
            ),
            ("ppt/media/image1.png".into(), b"\x89PNG fake".to_vec()),
            ("ppt/media/image2.emf".into(), b"emf".to_vec()),
        ];
        for (name, bytes) in entries {
            zip.start_file(name, opts).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn extract_pptx_visuals_copies_raster_images_of_visual_slides() {
        let dir = tempfile::tempdir().unwrap();
        let pptx = dir.path().join("deck.pptx");
        write_pptx_fixture(&pptx);
        let ws = dir.path().join("ws");

        let mut slides = process(extract_pptx(&pptx).unwrap());
        assert_eq!(slides.len(), 3);
        assert!(!slides[0].visual);
        assert!(slides[1].visual);
        assert!(slides[2].visual, "sparse before pictures are checked");

        let mut budget = 10;
        extract_pptx_visuals(&pptx, &mut slides, &ws, "deck", &mut budget);
        assert!(slides[0].images.is_empty());
        assert!(
            !slides[2].visual,
            "a sparse slide with no pictures is not visual"
        );
        assert!(slides[2].images.is_empty());
        assert_eq!(slides[1].images, vec!["_images/deck-s2-1.png"]);
        assert_eq!(budget, 9);
        assert_eq!(
            std::fs::read(ws.join("_images/deck-s2-1.png")).unwrap(),
            b"\x89PNG fake"
        );
    }

    #[test]
    fn extract_pptx_visuals_respects_budget() {
        let dir = tempfile::tempdir().unwrap();
        let pptx = dir.path().join("deck.pptx");
        write_pptx_fixture(&pptx);
        let mut slides = process(extract_pptx(&pptx).unwrap());
        let mut budget = 0;
        extract_pptx_visuals(&pptx, &mut slides, dir.path(), "deck", &mut budget);
        assert!(slides[1].images.is_empty());
        assert!(slides[1].visual, "over the cap, the slide keeps its marker");
        assert!(!dir.path().join(IMAGES_DIR).exists());
    }

    #[test]
    fn render_pdf_visuals_missing_tool_leaves_markers() {
        let dir = tempfile::tempdir().unwrap();
        let mut slides = vec![visual_slide(1, &[], &[])];
        let mut budget = 5;
        render_pdf_visuals_with(
            "csnotes-no-such-pdftoppm",
            &dir.path().join("deck.pdf"),
            &mut slides,
            dir.path(),
            "deck",
            &mut budget,
        );
        assert!(slides[0].images.is_empty());
        assert_eq!(budget, 5);
        assert!(to_workspace_markdown(&slides).contains("no image available"));
    }

    /// A one-page PDF with no text: the shape of an image-only slide.
    pub(crate) const BLANK_PDF: &str = "%PDF-1.4
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 150] >> endobj
trailer << /Root 1 0 R >>
%%EOF
";

    pub(crate) fn have(tool: &str) -> bool {
        std::process::Command::new(tool)
            .arg("-v")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    }

    #[test]
    fn blank_pdf_page_is_rendered_and_marked() {
        if !have("pdftotext") || !have("pdftoppm") {
            eprintln!("skipping: poppler not installed");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("deck.pdf");
        std::fs::write(&pdf, BLANK_PDF).unwrap();

        let mut slides = process(extract_pdf(&pdf).unwrap());
        assert_eq!(slides.len(), 1);
        assert!(slides[0].visual);

        let mut budget = 3;
        render_pdf_visuals(&pdf, &mut slides, dir.path(), "deck", &mut budget);
        assert_eq!(slides[0].images, vec!["_images/deck-p1.png"]);
        assert_eq!(budget, 2);
        let png = std::fs::read(dir.path().join("_images/deck-p1.png")).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}
