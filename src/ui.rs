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

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
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
