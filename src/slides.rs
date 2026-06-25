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
/// 4. **Formatting** — remaining slides are rendered to Markdown and returned
///    as a single string ready for XML-wrapping into the workspace.
///
/// Extraction failures (tool not found, corrupt archive) return a `SlideError`
/// whose message is shown to the user as a soft warning; the session continues
/// without the artifact.
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;

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
pub fn process(slides: Vec<RawSlide>) -> Vec<Slide> {
    if slides.is_empty() {
        return Vec::new();
    }

    // Phase 1: fuzzy dedup with a sliding window.
    // Repeated footer lines increase pairwise similarity, making it easier to
    // collapse near-identical slides (same code, varying highlights).
    let mut deduped: Vec<RawSlide> = Vec::with_capacity(slides.len());
    for slide in slides {
        let sig = word_set_signature(&slide.lines);
        if sig.is_empty() {
            continue; // silently drop already-empty slides
        }
        let is_dup = deduped
            .iter()
            .rev()
            .take(DEDUP_WINDOW)
            .any(|prev| jaccard(&sig, &word_set_signature(&prev.lines)) >= DEDUP_SIMILARITY);
        if !is_dup {
            deduped.push(slide);
        }
    }

    // Phase 2: strip boilerplate from the deduplicated set.
    // Frequency is now computed over unique slides only, so genuinely repeated
    // content (code shown 3× for emphasis, deduplicated above to 1 slide)
    // cannot be mistakenly flagged as per-deck boilerplate.
    let boilerplate = find_boilerplate(&deduped);
    deduped
        .into_iter()
        .map(|s| Slide {
            number: s.number,
            lines: s
                .lines
                .into_iter()
                .filter(|l| !boilerplate.contains(&normalise_line(l)))
                .filter(|l| !l.trim().is_empty())
                .collect(),
            notes: s.notes,
        })
        .filter(|s| !s.lines.is_empty())
        .collect()
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
/// per-slide `RawSlide` values.
fn pages_to_raw_slides(text: String) -> Vec<RawSlide> {
    text.split('\x0c')
        .enumerate()
        .filter_map(|(i, page)| {
            let lines: Vec<String> = page
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            if lines.is_empty() {
                None // trailing form-feed produces empty last page
            } else {
                Some(RawSlide {
                    number: i + 1,
                    lines,
                    notes: None,
                })
            }
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

/// Extract the text content of all `<a:t>` elements from a PowerPoint XML
/// string using `quick-xml`.  Preserves run order within each paragraph but
/// joins runs with no separator (PowerPoint splits runs on formatting
/// boundaries, not word boundaries).
fn extract_xml_text_nodes(xml: &str) -> Vec<String> {
    use quick_xml::escape::unescape;
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
                b"a:p" => {
                    current_para = Some(String::new());
                }
                b"a:t" => {
                    in_text = true;
                }
                _ => {}
            },
            Ok(Event::End(ref e)) => match e.name().as_ref() {
                b"a:p" => {
                    if let Some(para) = current_para.take() {
                        let trimmed = para.trim().to_string();
                        if !trimmed.is_empty() {
                            lines.push(trimmed);
                        }
                    }
                }
                b"a:t" => {
                    in_text = false;
                }
                _ => {}
            },
            Ok(Event::Text(e)) => {
                if in_text {
                    if let Some(ref mut para) = current_para {
                        if let Ok(decoded) = e.decode() {
                            match unescape(&decoded) {
                                Ok(text) => para.push_str(&text),
                                Err(_) => para.push_str(&decoded),
                            }
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
mod tests {
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
    fn process_empty_slide_after_strip_is_dropped() {
        // A slide consisting entirely of boilerplate lines becomes empty after
        // stripping and should be silently removed.
        let slides = vec![
            raw(1, &["Real content here", "© Footer"]),
            raw(2, &["© Footer"]), // becomes empty after stripping
            raw(3, &["More content", "© Footer"]),
            raw(4, &["Even more content", "© Footer"]),
        ];
        let result = process(slides);
        // Slide 2 should be dropped; others kept.
        assert!(
            result.iter().all(|s| s.number != 2),
            "empty slide after stripping must be dropped"
        );
        assert_eq!(result.len(), 3);
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

    // ── to_workspace_markdown ─────────────────────────────────────────────────

    #[test]
    fn to_workspace_markdown_formats_slide_with_notes() {
        let slides = vec![Slide {
            number: 3,
            lines: vec!["Quicksort".to_string(), "O(n log n) average".to_string()],
            notes: Some("Mention worst-case O(n²) on sorted input.".to_string()),
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
}
