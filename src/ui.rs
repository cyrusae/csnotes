/// Terminal color and progress helpers.
///
/// Call `init_color()` once at startup. All color in this codebase flows
/// through the `Paint` trait (for styled text) and `rainbow()` (for the HSL
/// gradient header). Both print plain text when `NO_COLOR` is set,
/// `TERM=dumb`, or stdout or stderr is not a tty.
///
/// `spinner()` shows an activity indicator on stderr for work that can take
/// more than a moment.  Output printed while one may be active goes through
/// `warn_line()` / `suspend()` so it doesn't collide with the spinner line.
use std::fmt;
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// Whether styled output emits ANSI escapes.  Off until `init_color()`
/// decides otherwise, so tests and library callers get plain text.
static COLOR: AtomicBool = AtomicBool::new(false);

/// Turn color on if the environment supports it. Must be called before any
/// output is produced.
pub fn init_color() {
    COLOR.store(color_supported(), Ordering::Relaxed);
}

/// True when the terminal appears to support ANSI true-color output.
/// Both streams must be terminals: either one may carry styled text, and a
/// redirected one (`| less`, `2> log`) should get plain text.
pub fn color_supported() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map_or(true, |t| t != "dumb")
        && std::io::stdout().is_terminal()
        && std::io::stderr().is_terminal()
}

fn color_enabled() -> bool {
    COLOR.load(Ordering::Relaxed)
}

/// Text styled by a `Paint` method.  Displays with ANSI escapes only when
/// color is enabled.  (`owo_colors::OwoColorize` always emits escapes; its
/// global override only applies to `if_supports_color`.)
pub struct Painted<'a, T: ?Sized> {
    inner: &'a T,
    style: owo_colors::Style,
}

impl<T: fmt::Display + ?Sized> fmt::Display for Painted<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if color_enabled() {
            write!(f, "{}", self.style.style(self.inner))
        } else {
            fmt::Display::fmt(self.inner, f)
        }
    }
}

/// Color-aware styling for anything `Display`; chain like `OwoColorize`
/// (`"x".red().bold()`).
pub trait Paint: fmt::Display {
    fn bold(&self) -> Painted<'_, Self> {
        self.painted(owo_colors::Style::new().bold())
    }
    fn dimmed(&self) -> Painted<'_, Self> {
        self.painted(owo_colors::Style::new().dimmed())
    }
    fn green(&self) -> Painted<'_, Self> {
        self.painted(owo_colors::Style::new().green())
    }
    fn red(&self) -> Painted<'_, Self> {
        self.painted(owo_colors::Style::new().red())
    }
    fn yellow(&self) -> Painted<'_, Self> {
        self.painted(owo_colors::Style::new().yellow())
    }
    #[doc(hidden)]
    fn painted(&self, style: owo_colors::Style) -> Painted<'_, Self> {
        Painted { inner: self, style }
    }
}

impl<T: fmt::Display + ?Sized> Paint for T {}

/// Apply a rainbow HSL gradient (0°–300°) to `s` using ANSI true-color
/// escape codes. Returns a plain string when color is disabled.
pub fn rainbow(s: &str) -> String {
    if !color_enabled() {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len().max(1);
    let mut out = String::with_capacity(s.len() + chars.len() * 22);
    for (i, ch) in chars.iter().enumerate() {
        let hue = (i as f64 / n as f64) * 300.0;
        let (r, g, b) = hsl_to_rgb(hue, 1.0, 0.62);
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m{ch}"));
    }
    out.push_str("\x1b[0m");
    out
}

pub(crate) fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    (
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

// ── Spinners ──────────────────────────────────────────────────────────────────

/// The spinner currently in scope, if any.  Only one is live at a time; a
/// spinner started while another is active is inert.  The lock also orders
/// the delayed first draw against `Drop`, so a finished spinner never draws.
static ACTIVE: Mutex<Option<ProgressBar>> = Mutex::new(None);

/// Work that finishes sooner than this never shows a spinner (no flicker).
const SPINNER_DELAY: Duration = Duration::from_millis(250);
const SPINNER_TICK: Duration = Duration::from_millis(100);

/// RAII guard for `spinner()`: the spinner is cleared when this is dropped.
#[must_use = "the spinner disappears when this guard is dropped"]
pub struct Spinner {
    bar: Option<ProgressBar>,
}

impl Spinner {
    /// A guard that shows nothing.
    pub fn inert() -> Self {
        Spinner { bar: None }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if let Some(bar) = self.bar.take() {
            let mut active = lock_active();
            bar.finish_and_clear();
            *active = None;
        }
    }
}

/// True when stderr is an interactive terminal that can redraw a line.
pub fn spinners_enabled() -> bool {
    std::io::stderr().is_terminal() && std::env::var("TERM").map_or(true, |t| t != "dumb")
}

/// Start a spinner reading `message` on stderr.  It appears only if the
/// guard is still alive after a short delay, and only on a terminal.
/// Never hold one across a prompt or while an AI backend owns the terminal.
pub fn spinner(message: impl Into<String>) -> Spinner {
    spinner_with(spinners_enabled(), message.into())
}

/// Like `spinner()`, but when spinners are unavailable (not a terminal),
/// print `message` once instead so non-interactive output still says why
/// the command is pausing.
pub fn spinner_or_note(message: impl Into<String>) -> Spinner {
    let message = message.into();
    if spinners_enabled() {
        spinner_with(true, message)
    } else {
        eprintln!("{}", message.dimmed());
        Spinner::inert()
    }
}

fn spinner_with(enabled: bool, message: String) -> Spinner {
    if !enabled {
        return Spinner::inert();
    }
    let mut active = lock_active();
    if active.is_some() {
        return Spinner::inert();
    }
    let bar = ProgressBar::with_draw_target(None, ProgressDrawTarget::hidden());
    bar.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    bar.set_message(message);
    *active = Some(bar.clone());
    drop(active);

    let delayed = bar.clone();
    std::thread::spawn(move || {
        std::thread::sleep(SPINNER_DELAY);
        let _active = lock_active();
        if !delayed.is_finished() {
            delayed.set_draw_target(ProgressDrawTarget::stderr());
            delayed.enable_steady_tick(SPINNER_TICK);
        }
    });
    Spinner { bar: Some(bar) }
}

/// Run `f` (which prints) with any active spinner temporarily cleared.
/// `f` must not start a spinner.
pub fn suspend<R>(f: impl FnOnce() -> R) -> R {
    let active = lock_active();
    match active.as_ref() {
        Some(bar) => bar.suspend(f),
        None => f(),
    }
}

/// `eprintln!` that steps around an active spinner.
pub fn warn_line(line: impl std::fmt::Display) {
    suspend(|| eprintln!("{line}"));
}

fn lock_active() -> std::sync::MutexGuard<'static, Option<ProgressBar>> {
    // A panic while printing can't leave the slot inconsistent; keep going.
    ACTIVE.lock().unwrap_or_else(|e| e.into_inner())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Spinner tests share the global slot; run them one at a time.
    static SPINNER_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// Paint tests flip the global color switch; run them one at a time.
    static COLOR_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn paint_is_plain_when_color_disabled() {
        let _g = COLOR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        COLOR.store(false, Ordering::Relaxed);
        assert_eq!("reconcile".dimmed().to_string(), "reconcile");
        assert_eq!(format!("{}", "err".red().bold()), "err");
        assert_eq!(rainbow("hi"), "hi");
    }

    #[test]
    fn paint_styles_when_color_enabled() {
        let _g = COLOR_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        COLOR.store(true, Ordering::Relaxed);
        let dim = "x".dimmed().to_string();
        let chained = 3.red().bold().to_string();
        COLOR.store(false, Ordering::Relaxed);
        assert_eq!(dim, "\x1b[2mx\x1b[0m");
        assert!(chained.contains("\x1b[1m") && chained.contains("\x1b[31m"));
        assert!(chained.contains('3'));
    }

    #[test]
    fn spinner_disabled_is_inert() {
        let _g = SPINNER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let s = spinner_with(false, "working".into());
        assert!(s.bar.is_none());
        assert!(lock_active().is_none());
    }

    #[test]
    fn spinner_registers_and_clears_on_drop() {
        let _g = SPINNER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let s = spinner_with(true, "working".into());
        assert!(s.bar.is_some());
        assert!(lock_active().is_some());
        drop(s);
        assert!(lock_active().is_none());
    }

    #[test]
    fn nested_spinner_is_inert() {
        let _g = SPINNER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let outer = spinner_with(true, "outer".into());
        let inner = spinner_with(true, "inner".into());
        assert!(inner.bar.is_none());
        drop(inner);
        assert!(lock_active().is_some(), "inner drop must not clear outer");
        drop(outer);
        assert!(lock_active().is_none());
    }

    #[test]
    fn spinner_dropped_before_delay_never_draws() {
        let _g = SPINNER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let s = spinner_with(true, "quick".into());
        let bar = s.bar.clone().unwrap();
        drop(s);
        std::thread::sleep(SPINNER_DELAY + Duration::from_millis(100));
        assert!(bar.is_finished());
        assert!(
            bar.is_hidden(),
            "finished spinner must not attach to stderr"
        );
    }

    #[test]
    fn suspend_runs_closure_with_and_without_spinner() {
        let _g = SPINNER_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(suspend(|| 1), 1);
        let s = spinner_with(true, "working".into());
        assert_eq!(suspend(|| 2), 2);
        drop(s);
    }

    // hsl_to_rgb: one test per branch (h<60, 60-120, 120-180, 180-240, 240-300, ≥300)
    // using s=1.0, l=0.5 so m=0 and c=1, giving clean integer-ish values.

    #[test]
    fn hsl_red_region_h30() {
        // h=30 → branch h<60: (c, x, 0), x≈0.5
        let (r, g, b) = hsl_to_rgb(30.0, 1.0, 0.5);
        assert_eq!(r, 255);
        assert_eq!(g, 128);
        assert_eq!(b, 0);
    }

    #[test]
    fn hsl_yellow_region_h90() {
        // h=90 → branch 60<=h<120: (x, c, 0), x≈0.5
        let (r, g, b) = hsl_to_rgb(90.0, 1.0, 0.5);
        assert_eq!(r, 128);
        assert_eq!(g, 255);
        assert_eq!(b, 0);
    }

    #[test]
    fn hsl_green_region_h150() {
        // h=150 → branch 120<=h<180: (0, c, x), x≈0.5
        let (r, g, b) = hsl_to_rgb(150.0, 1.0, 0.5);
        assert_eq!(r, 0);
        assert_eq!(g, 255);
        assert_eq!(b, 128);
    }

    #[test]
    fn hsl_cyan_region_h210() {
        // h=210 → branch 180<=h<240: (0, x, c), x≈0.5
        let (r, g, b) = hsl_to_rgb(210.0, 1.0, 0.5);
        assert_eq!(r, 0);
        assert_eq!(g, 128);
        assert_eq!(b, 255);
    }

    #[test]
    fn hsl_blue_region_h270() {
        // h=270 → branch 240<=h<300: (x, 0, c), x≈0.5
        let (r, g, b) = hsl_to_rgb(270.0, 1.0, 0.5);
        assert_eq!(r, 128);
        assert_eq!(g, 0);
        assert_eq!(b, 255);
    }

    #[test]
    fn hsl_magenta_region_h330() {
        // h=330 → else branch (h>=300): (c, 0, x)
        // x = 1*(1-|(330/60)%2-1|) = 1*(1-|5.5%2-1|) = 1*(1-|1.5-1|) = 0.5
        let (r, g, b) = hsl_to_rgb(330.0, 1.0, 0.5);
        assert_eq!(r, 255);
        assert_eq!(g, 0);
        assert_eq!(b, 128);
    }

    #[test]
    fn hsl_pure_red_h0() {
        // Boundary: h=0 (same branch as h=30)
        let (r, g, b) = hsl_to_rgb(0.0, 1.0, 0.5);
        assert_eq!(r, 255);
        assert_eq!(b, 0);
        // g depends on x=c*(1-|0-1|)=0
        assert_eq!(g, 0);
    }

    #[test]
    fn hsl_high_lightness_exposes_plus_minus_mutations() {
        // l=0.75, s=1.0: c=(1-|2*0.75-1|)*1=0.5, x=0 (h=0 sector), m=0.75-0.25=0.5
        // r=(0.5+0.5)*255=255, g=(0+0.5)*255≈128, b=(0+0.5)*255≈128
        // Catches +→- at the `r1+m`, `g1+m`, `b1+m` additions.
        let (r, g, b) = hsl_to_rgb(0.0, 1.0, 0.75);
        assert_eq!(r, 255);
        assert_eq!(g, 128);
        assert_eq!(b, 128);
    }

    #[test]
    fn hsl_fractional_saturation_exposes_multiply_mutations() {
        // l=0.5, s=0.5: c=(1-0)*0.5=0.5, x=0 (h=0 sector), m=0.5-0.25=0.25
        // r=(0.5+0.25)*255≈191, g=(0+0.25)*255≈64, b=(0+0.25)*255≈64
        // Catches *→/ in the `c*(1-...)` and `c*x` expressions since s≠1.
        let (r, g, b) = hsl_to_rgb(0.0, 0.5, 0.5);
        assert_eq!(r, 191);
        assert_eq!(g, 64);
        assert_eq!(b, 64);
    }

    // rainbow: in test context stdout is not a tty, so color_supported() → false
    // and rainbow() must return the plain string unchanged.

    #[test]
    fn rainbow_returns_plain_string_when_color_not_supported() {
        // This holds whenever stdout is not a terminal (true in test harness).
        if !color_supported() {
            assert_eq!(rainbow("hello"), "hello");
            assert_eq!(rainbow(""), "");
        }
    }

    #[test]
    fn rainbow_single_char_no_panic() {
        let _ = rainbow("x");
    }

    #[test]
    fn rainbow_empty_no_panic() {
        let _ = rainbow("");
    }

    // color_supported: verify env-var gates without mutating stdout.

    #[test]
    fn color_supported_false_when_no_color_set() {
        // Set NO_COLOR; color_supported must return false regardless of tty.
        // Safety: env mutation in tests is process-global; run sequentially.
        unsafe { std::env::set_var("NO_COLOR", "1") };
        let result = color_supported();
        unsafe { std::env::remove_var("NO_COLOR") };
        assert!(!result);
    }

    #[test]
    fn color_supported_false_when_term_dumb() {
        let old = std::env::var("TERM").ok();
        unsafe { std::env::set_var("TERM", "dumb") };
        // Also clear NO_COLOR so only TERM drives the result.
        unsafe { std::env::remove_var("NO_COLOR") };
        let result = color_supported();
        match old {
            Some(v) => unsafe { std::env::set_var("TERM", v) },
            None => unsafe { std::env::remove_var("TERM") },
        }
        assert!(!result);
    }
}
