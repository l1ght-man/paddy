//! Color themes. A theme is plain data: eight colors, a name and a light/dark
//! flag. Themes come from the built-in set, from `*.theme` files in the themes
//! folder, or from an imported URL. Parsing is strict and size-capped, and a
//! theme whose text would be unreadable is rejected, so a bad or hostile file
//! can never make the UI unusable.
//!
//! File format (`#` starts a comment):
//! ```text
//! id = nord
//! name = Nord
//! dark = true
//! bg = #2e3440
//! ...
//! ```

use std::fmt;

/// Hard cap on a theme file (they are ~250 bytes).
pub const MAX_THEME_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` only (no names, no alpha, no shorthand): one unambiguous spelling.
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.strip_prefix('#')?;
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// WCAG relative luminance.
    fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// WCAG contrast ratio, 1.0 (identical) to 21.0 (black on white).
    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    /// Lowercase `a-z0-9-`, at most 32 chars. Also the file name: `<id>.theme`.
    pub id: String,
    pub name: String,
    /// Is the background dark? (Decides which built-in a light/dark toggle goes to.)
    pub dark: bool,
    pub bg: Rgb,
    pub panel: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    pub line: Rgb,
    pub accent: Rgb,
    pub sel: Rgb,
    pub warn: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    TooBig(usize),
    Missing(&'static str),
    Bad { key: String, why: String },
    Unreadable(String),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::TooBig(n) => write!(f, "theme file is {n} bytes; the limit is {MAX_THEME_BYTES}"),
            ThemeError::Missing(k) => write!(f, "theme is missing \"{k}\""),
            ThemeError::Bad { key, why } => write!(f, "bad \"{key}\": {why}"),
            ThemeError::Unreadable(m) => write!(f, "theme would be hard to read: {m}"),
        }
    }
}

impl std::error::Error for ThemeError {}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
}

fn bad(key: &str, why: &str) -> ThemeError {
    ThemeError::Bad { key: key.into(), why: why.into() }
}

impl Theme {
    pub fn parse(text: &str) -> Result<Theme, ThemeError> {
        if text.len() > MAX_THEME_BYTES {
            return Err(ThemeError::TooBig(text.len()));
        }
        let (mut id, mut name, mut dark) = (None, None, None);
        let mut colors: [Option<Rgb>; 8] = [None; 8];
        const KEYS: [&str; 8] = ["bg", "panel", "fg", "dim", "line", "accent", "sel", "warn"];
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "id" => {
                    if !valid_id(v) {
                        return Err(bad("id", "use 1-32 characters of a-z, 0-9 and -"));
                    }
                    id = Some(v.to_string());
                }
                "name" => {
                    if v.is_empty() || v.chars().count() > 40 || v.chars().any(char::is_control) {
                        return Err(bad("name", "1-40 printable characters"));
                    }
                    name = Some(v.to_string());
                }
                "dark" => {
                    dark = Some(match v {
                        "true" => true,
                        "false" => false,
                        _ => return Err(bad("dark", "must be true or false")),
                    })
                }
                other => {
                    if let Some(i) = KEYS.iter().position(|c| *c == other) {
                        colors[i] = Some(Rgb::parse(v).ok_or_else(|| bad(other, "expected #rrggbb"))?);
                    }
                    // unknown keys are ignored so themes can carry extra notes
                }
            }
        }
        let get = |i: usize| colors[i].ok_or(ThemeError::Missing(KEYS[i]));
        let t = Theme {
            id: id.ok_or(ThemeError::Missing("id"))?,
            name: name.ok_or(ThemeError::Missing("name"))?,
            dark: dark.ok_or(ThemeError::Missing("dark"))?,
            bg: get(0)?,
            panel: get(1)?,
            fg: get(2)?,
            dim: get(3)?,
            line: get(4)?,
            accent: get(5)?,
            sel: get(6)?,
            warn: get(7)?,
        };
        t.check_readable()?;
        Ok(t)
    }

    /// Reject palettes where text vanishes into the background.
    pub fn check_readable(&self) -> Result<(), ThemeError> {
        let need = [
            ("text", self.fg, self.bg, 4.5),
            ("text on panels", self.fg, self.panel, 4.0),
            ("dim text", self.dim, self.bg, 2.4),
            ("accent", self.accent, self.bg, 3.0),
            ("warning", self.warn, self.bg, 3.0),
            ("text on the selection", self.fg, self.sel, 3.5),
        ];
        for (what, a, b, min) in need {
            let c = a.contrast(b);
            if c < min {
                return Err(ThemeError::Unreadable(format!("{what} has contrast {c:.1}, needs at least {min}")));
            }
        }
        Ok(())
    }

    pub fn to_text(&self) -> String {
        format!(
            "# paddy theme\nid = {}\nname = {}\ndark = {}\nbg = {}\npanel = {}\nfg = {}\ndim = {}\nline = {}\naccent = {}\nsel = {}\nwarn = {}\n",
            self.id, self.name, self.dark, self.bg.hex(), self.panel.hex(), self.fg.hex(), self.dim.hex(),
            self.line.hex(), self.accent.hex(), self.sel.hex(), self.warn.hex()
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn t(id: &str, name: &str, dark: bool, c: [&str; 8]) -> Theme {
    let p = |s: &str| Rgb::parse(s).expect("built-in colors are valid");
    Theme {
        id: id.into(),
        name: name.into(),
        dark,
        bg: p(c[0]),
        panel: p(c[1]),
        fg: p(c[2]),
        dim: p(c[3]),
        line: p(c[4]),
        accent: p(c[5]),
        sel: p(c[6]),
        warn: p(c[7]),
    }
}

pub const DEFAULT_THEME: &str = "dark";
pub const DEFAULT_LIGHT: &str = "light";

/// Themes shipped in the binary (order = order in the picker). Colors are the
/// well-known public palettes of these schemes, mapped onto paddy's eight roles.
pub fn builtin() -> Vec<Theme> {
    vec![
        //          bg         panel      fg         dim        line       accent     sel        warn
        t(
            "dark",
            "dark (default)",
            true,
            ["#0a0b0b", "#0e100f", "#c5cac3", "#666d66", "#2a302b", "#8caa89", "#171d18", "#c98a6f"],
        ),
        t(
            "light",
            "light cream",
            false,
            ["#fcf8ee", "#f8f2e2", "#3b3936", "#857f70", "#dfd5bb", "#7b5a2b", "#f0e7d0", "#a4472e"],
        ),
        t(
            "nord",
            "Nord",
            true,
            ["#2e3440", "#3b4252", "#d8dee9", "#8b97ad", "#4c566a", "#88c0d0", "#434c5e", "#d08770"],
        ),
        t(
            "dracula",
            "Dracula",
            true,
            ["#282a36", "#21222c", "#f8f8f2", "#8a96c8", "#44475a", "#bd93f9", "#44475a", "#ff6e6e"],
        ),
        t(
            "gruvbox-dark",
            "Gruvbox dark",
            true,
            ["#282828", "#32302f", "#ebdbb2", "#a19484", "#504945", "#b8bb26", "#3c3836", "#fb4934"],
        ),
        t(
            "gruvbox-light",
            "Gruvbox light",
            false,
            ["#fbf1c7", "#f2e5bc", "#3c3836", "#75695f", "#d5c4a1", "#79740e", "#ebdbb2", "#9d0006"],
        ),
        t(
            "solarized-dark",
            "Solarized dark",
            true,
            ["#002b36", "#01313c", "#93a1a1", "#6b8189", "#0f4855", "#2aa198", "#073642", "#dc6e3c"],
        ),
        t(
            "solarized-light",
            "Solarized light",
            false,
            ["#fdf6e3", "#eee8d5", "#586e75", "#8a999a", "#d8d0b8", "#1f8a82", "#e6dfc8", "#cb3d3a"],
        ),
        t(
            "tokyo-night",
            "Tokyo Night",
            true,
            ["#1a1b26", "#16161e", "#c0caf5", "#6f7aa8", "#292e42", "#7aa2f7", "#292e42", "#f7768e"],
        ),
        t(
            "catppuccin",
            "Catppuccin Mocha",
            true,
            ["#1e1e2e", "#181825", "#cdd6f4", "#8a90a8", "#313244", "#cba6f7", "#313244", "#f38ba8"],
        ),
        t(
            "one-dark",
            "One Dark",
            true,
            ["#282c34", "#21252b", "#abb2bf", "#7a8291", "#3e4451", "#98c379", "#2c313a", "#e06c75"],
        ),
        t(
            "high-contrast",
            "High contrast",
            true,
            ["#000000", "#0a0a0a", "#ffffff", "#bdbdbd", "#6b6b6b", "#ffff00", "#1f1f1f", "#ff7b7b"],
        ),
    ]
}

/// Look a theme up by id among the given lists (later lists win: user themes shadow built-ins).
pub fn find<'a>(id: &str, lists: &[&'a [Theme]]) -> Option<&'a Theme> {
    lists.iter().rev().find_map(|l| l.iter().find(|t| t.id == id))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORD: &str = "id = mine\nname = Mine\ndark = true\nbg = #000000\npanel = #101010\nfg = #ffffff\ndim = #aaaaaa\nline = #333333\naccent = #00ff00\nsel = #202020\nwarn = #ff5555\n";

    #[test]
    fn builtin_themes_are_valid_readable_and_unique() {
        let all = builtin();
        assert!(all.len() >= 10);
        let mut ids = std::collections::HashSet::new();
        for t in &all {
            assert!(valid_id(&t.id), "{}", t.id);
            assert!(ids.insert(t.id.clone()), "duplicate id {}", t.id);
            t.check_readable().unwrap_or_else(|e| panic!("{}: {e}", t.id));
            assert_eq!(Theme::parse(&t.to_text()).as_ref(), Ok(t), "{} round-trips", t.id);
            assert_eq!(t.dark, t.bg.luminance() < 0.3, "{}: dark flag matches its background", t.id);
        }
        assert!(all.iter().any(|t| t.id == DEFAULT_THEME) && all.iter().any(|t| t.id == DEFAULT_LIGHT));
    }

    #[test]
    fn parses_a_good_theme() {
        let t = Theme::parse(NORD).unwrap();
        assert_eq!((t.id.as_str(), t.name.as_str(), t.dark), ("mine", "Mine", true));
        assert_eq!(t.accent, Rgb(0, 255, 0));
        // comments, blank lines, unknown keys and spacing are fine
        let noisy = format!("# hi\n\n  extra = whatever  \n{NORD}\nnote: not a pair\n");
        assert_eq!(Theme::parse(&noisy).unwrap(), t);
    }

    #[test]
    fn rejects_bad_input_with_a_reason() {
        let with = |from: &str, to: &str| NORD.replace(from, to);
        for (text, needle) in [
            (with("bg = #000000", "bg = black"), "expected #rrggbb"),
            (with("bg = #000000", "bg = #000"), "expected #rrggbb"),
            (with("bg = #000000", "bg = #00000g"), "expected #rrggbb"),
            (with("bg = #000000", ""), "missing \"bg\""),
            (with("id = mine", "id = ../evil"), "bad \"id\""),
            (with("id = mine", "id = Mine"), "bad \"id\""),
            (with("id = mine", "id = -x"), "bad \"id\""),
            (with("name = Mine", "name = "), "bad \"name\""),
            (with("name = Mine", "name = a\u{7}b"), "bad \"name\""),
            (with("dark = true", "dark = yes"), "bad \"dark\""),
            ("x".repeat(MAX_THEME_BYTES + 1), "limit"),
            (String::new(), "missing"),
        ] {
            let err = Theme::parse(&text).unwrap_err().to_string();
            assert!(err.contains(needle), "{needle:?} not in {err:?}");
        }
    }

    #[test]
    fn unreadable_themes_are_refused() {
        // text the same color as the background
        let t = NORD.replace("fg = #ffffff", "fg = #010101");
        assert!(matches!(Theme::parse(&t), Err(ThemeError::Unreadable(_))));
        // invisible accent
        let t = NORD.replace("accent = #00ff00", "accent = #050505");
        assert!(Theme::parse(&t).unwrap_err().to_string().contains("accent"));
    }

    #[test]
    fn contrast_math() {
        assert!((Rgb(0, 0, 0).contrast(Rgb(255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((Rgb(10, 10, 10).contrast(Rgb(10, 10, 10)) - 1.0).abs() < 1e-9);
        assert_eq!(Rgb::parse("#8CAA89"), Some(Rgb(0x8c, 0xaa, 0x89)));
        assert_eq!(Rgb(1, 2, 3).hex(), "#010203");
        assert_eq!(Rgb::parse("8caa89"), None);
        assert_eq!(Rgb::parse("#8caa89ff"), None);
    }

    #[test]
    fn user_themes_shadow_builtins() {
        let b = builtin();
        let mine = vec![Theme { name: "My dark".into(), ..b[0].clone() }];
        assert_eq!(find("dark", &[&b, &mine]).unwrap().name, "My dark");
        assert_eq!(find("nord", &[&b, &mine]).unwrap().name, "Nord");
        assert!(find("nope", &[&b]).is_none());
    }

    #[test]
    fn arbitrary_text_never_panics() {
        // cheap fuzz: odd unicode, huge lines, stray separators
        for s in [
            "=",
            "===",
            "id",
            "\u{0}",
            "bg = #\u{202e}ffffff",
            "名前 = 値",
            &"a=".repeat(1000),
            "id = é\nname = ñ\ndark = true",
        ] {
            let _ = Theme::parse(s);
        }
    }
}
