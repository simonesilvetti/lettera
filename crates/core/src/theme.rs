//! Theme tokens: built-in dark/light palettes and the Omarchy `colors.toml` reader.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tokens {
    pub bg: String,
    pub fg: String,
    pub muted: String,
    pub border: String,
    pub surface: String,
    pub accent: String,
    pub red: String,
    pub green: String,
}

/// What the UI needs to paint itself. `tokens = None` means "follow
/// `prefers-color-scheme` with the built-in palettes" (they live in app.css too).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemePayload {
    pub tokens: Option<Tokens>,
    pub font: String,
    pub font_size: u32,
}

fn tokens(v: [&str; 8]) -> Tokens {
    Tokens {
        bg: v[0].into(),
        fg: v[1].into(),
        muted: v[2].into(),
        border: v[3].into(),
        surface: v[4].into(),
        accent: v[5].into(),
        red: v[6].into(),
        green: v[7].into(),
    }
}

pub fn dark() -> Tokens {
    tokens([
        "#1a1b26", "#c0caf5", "#8089b3", "#292e42", "#24283b", "#7aa2f7", "#f7768e", "#9ece6a",
    ])
}

pub fn light() -> Tokens {
    tokens([
        "#ffffff", "#1a1a1a", "#666666", "#e6e6e6", "#f3f3f3", "#2f6fed", "#c62828", "#2e7d32",
    ])
}

/// Candidate locations of the current Omarchy palette, in priority order.
pub fn omarchy_paths() -> Vec<PathBuf> {
    let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()) else {
        return Vec::new();
    };
    vec![
        home.join(".local/state/omarchy/current/theme/colors.toml"),
        home.join(".config/omarchy/current/theme/colors.toml"),
    ]
}

/// Directories to watch for theme switches (the theme dir is replaced wholesale).
pub fn omarchy_watch_dirs() -> Vec<PathBuf> {
    omarchy_paths()
        .iter()
        .filter_map(|p| p.parent().and_then(Path::parent).map(Path::to_path_buf))
        .filter(|d| d.exists())
        .collect()
}

/// Parses `#rgb`, `#rrggbb`, `#rrggbbaa`, `rrggbb` and `0xrrggbb` into (r, g, b).
pub fn parse_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim();
    let hex = s
        .strip_prefix('#')
        .or_else(|| s.strip_prefix("0x"))
        .unwrap_or(s);
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    match hex.len() {
        3 => {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
            Some((d(0)?, d(1)?, d(2)?))
        }
        6 | 8 => Some((byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `p` of `a` over `1 - p` of `b`.
pub fn mix(a: (u8, u8, u8), b: (u8, u8, u8), p: f32) -> String {
    let m = |x: u8, y: u8| {
        (x as f32 * p + y as f32 * (1.0 - p))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    hex((m(a.0, b.0), m(a.1, b.1), m(a.2, b.2)))
}

/// Maps an Omarchy `colors.toml` to tokens. Returns `None` (and logs) when the
/// file is unusable; unknown keys are ignored.
pub fn parse_omarchy(text: &str) -> Option<Tokens> {
    let table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("invalid colors.toml: {e}");
            return None;
        }
    };
    let color = |key: &str| {
        table
            .get(key)
            .and_then(|v| v.as_str())
            .and_then(parse_color)
    };
    let (Some(bg), Some(fg)) = (color("background"), color("foreground")) else {
        tracing::warn!("colors.toml senza background/foreground");
        return None;
    };
    let fallback = dark();
    let pick = |keys: &[&str], default: &str| {
        keys.iter()
            .find_map(|k| color(k))
            .map(hex)
            .unwrap_or_else(|| default.to_string())
    };
    let muted = ["muted", "comment", "dim_foreground", "dim"]
        .iter()
        .find_map(|k| color(k))
        .map(hex)
        .unwrap_or_else(|| mix(fg, bg, 0.55));
    Some(Tokens {
        bg: hex(bg),
        fg: hex(fg),
        muted,
        border: mix(fg, bg, 0.12),
        surface: mix(fg, bg, 0.06),
        accent: pick(&["accent", "blue"], &fallback.accent),
        red: pick(&["red"], &fallback.red),
        green: pick(&["green"], &fallback.green),
    })
}

pub fn read_omarchy() -> Option<Tokens> {
    for path in omarchy_paths() {
        match std::fs::read_to_string(&path) {
            Ok(text) => return parse_omarchy(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::warn!("reading {} failed: {e}", path.display());
                continue;
            }
        }
    }
    None
}

pub fn resolve(theme: &str, font: &str, font_size: u32) -> ThemePayload {
    let tokens = match theme {
        "dark" => Some(dark()),
        "light" => Some(light()),
        "auto" | "omarchy" => read_omarchy(),
        other => {
            tracing::warn!("unknown theme {other:?}, using auto");
            read_omarchy()
        }
    };
    ThemePayload {
        tokens,
        font: font.to_string(),
        font_size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_color("#1a1b26"), Some((0x1a, 0x1b, 0x26)));
        assert_eq!(parse_color("1a1b26"), Some((0x1a, 0x1b, 0x26)));
        assert_eq!(parse_color("0x1a1b26"), Some((0x1a, 0x1b, 0x26)));
        assert_eq!(parse_color("#1a1b26ff"), Some((0x1a, 0x1b, 0x26)));
        assert_eq!(parse_color("#12"), None);
        assert_eq!(parse_color("rgb(1,2,3)"), None);
        assert_eq!(parse_color("#ggg"), None);
        assert_eq!(parse_color("#ééé"), None);
    }

    #[test]
    fn mixing() {
        assert_eq!(mix((255, 255, 255), (0, 0, 0), 0.5), "#808080");
        assert_eq!(mix((255, 255, 255), (0, 0, 0), 0.0), "#000000");
        assert_eq!(mix((255, 255, 255), (0, 0, 0), 1.0), "#ffffff");
    }

    #[test]
    fn full_omarchy_file() {
        let t = parse_omarchy(
            r##"
mode = "dark"
accent = "#faa968"
muted = "#2a6b78"
background = "#05182e"
foreground = "#f6dcac"
red = "#f85525"
green = "#028391"
unknown_key = 42
"##,
        )
        .unwrap();
        assert_eq!(t.bg, "#05182e");
        assert_eq!(t.fg, "#f6dcac");
        assert_eq!(t.accent, "#faa968");
        assert_eq!(t.muted, "#2a6b78");
        assert_eq!(t.red, "#f85525");
        assert_eq!(t.green, "#028391");
        let fg = parse_color("#f6dcac").unwrap();
        let bg = parse_color("#05182e").unwrap();
        assert_eq!(t.border, mix(fg, bg, 0.12));
        assert_eq!(t.surface, mix(fg, bg, 0.06));
    }

    #[test]
    fn minimal_file_computes_muted_and_falls_back() {
        let t = parse_omarchy("background = \"#000000\"\nforeground = \"#ffffff\"\n").unwrap();
        assert_eq!(t.muted, mix((255, 255, 255), (0, 0, 0), 0.55));
        assert_eq!(t.accent, dark().accent);
        assert_eq!(t.red, dark().red);
    }

    #[test]
    fn comment_key_used_as_muted_and_blue_as_accent() {
        let t = parse_omarchy(
            "background = \"#000\"\nforeground = \"#fff\"\ncomment = \"#777\"\nblue = \"#00f\"\n",
        )
        .unwrap();
        assert_eq!(t.muted, "#777777");
        assert_eq!(t.accent, "#0000ff");
    }

    #[test]
    fn broken_files_never_panic() {
        assert!(parse_omarchy("this is = = not toml").is_none());
        assert!(parse_omarchy("").is_none());
        assert!(parse_omarchy("background = 3\nforeground = \"#fff\"").is_none());
        assert!(parse_omarchy("background = \"nope\"\nforeground = \"#fff\"").is_none());
        // a bad optional key falls back instead of failing
        let t = parse_omarchy("background = \"#000\"\nforeground = \"#fff\"\nred = [1]").unwrap();
        assert_eq!(t.red, dark().red);
    }

    #[test]
    fn explicit_themes() {
        assert_eq!(resolve("dark", "f", 13).tokens, Some(dark()));
        assert_eq!(resolve("light", "f", 13).tokens, Some(light()));
    }
}
