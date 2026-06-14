/// Terminal color helpers.
///
/// Call `init_color()` once at startup. All color in this codebase flows
/// through `owo_colors::OwoColorize` (for styled text) and `rainbow()` (for
/// the HSL gradient header). Both degrade gracefully when `NO_COLOR` is set,
/// `TERM=dumb`, or stdout is not a tty.
use std::io::IsTerminal;

/// Configure owo-colors based on the current environment. Must be called
/// before any output is produced.
pub fn init_color() {
    if !color_supported() {
        owo_colors::set_override(false);
    }
}

/// True when the terminal appears to support ANSI true-color output.
pub fn color_supported() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map_or(true, |t| t != "dumb")
        && std::io::stdout().is_terminal()
}

/// Apply a rainbow HSL gradient (0°–300°) to `s` using ANSI true-color
/// escape codes. Returns a plain string when color is disabled.
pub fn rainbow(s: &str) -> String {
    if !color_supported() {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
