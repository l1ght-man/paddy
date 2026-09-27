//! App settings that live outside any vault: theme, hotkey, last vault.
//! Stored as plain `key = value` lines so it stays hand-editable.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// How much vertical breathing room list rows and inputs get.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Roomy,
}

impl Density {
    pub fn as_str(self) -> &'static str {
        match self {
            Density::Compact => "compact",
            Density::Normal => "normal",
            Density::Roomy => "roomy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "compact" => Some(Density::Compact),
            "normal" => Some(Density::Normal),
            "roomy" => Some(Density::Roomy),
            _ => None,
        }
    }

    /// Row height as a multiple of the font size.
    pub fn row_scale(self) -> f32 {
        match self {
            Density::Compact => 1.4,
            Density::Normal => 1.7,
            Density::Roomy => 2.1,
        }
    }

    /// Multiplier for gaps and padding between elements (1.0 = the normal 8 px gap).
    pub fn gap_scale(self) -> f32 {
        match self {
            Density::Compact => 0.4,
            Density::Normal => 1.0,
            Density::Roomy => 1.8,
        }
    }
}

pub const DEFAULT_FONT: &str = "DejaVu Sans Mono";
pub const MIN_FONT_SIZE: u8 = 10;
pub const MAX_FONT_SIZE: u8 = 22;
pub const DEFAULT_FONT_SIZE: u8 = 13;

/// Which title bar the main window uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowBar {
    /// Built-in bar under WSL (where the native one is unreliable), native elsewhere.
    #[default]
    Auto,
    Builtin,
    Native,
}

impl WindowBar {
    fn as_str(self) -> &'static str {
        match self {
            WindowBar::Auto => "auto",
            WindowBar::Builtin => "builtin",
            WindowBar::Native => "native",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(WindowBar::Auto),
            "builtin" => Some(WindowBar::Builtin),
            "native" => Some(WindowBar::Native),
            _ => None,
        }
    }

    /// Should paddy draw its own title bar?
    pub fn use_builtin(self) -> bool {
        match self {
            WindowBar::Builtin => true,
            WindowBar::Native => false,
            WindowBar::Auto => running_under_wsl(),
        }
    }
}

/// True when running inside Windows Subsystem for Linux.
pub fn running_under_wsl() -> bool {
    std::env::var_os("WSL_DISTRO_NAME").is_some()
        || Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
        || Path::new("/run/WSL").exists()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Id of the active theme (a built-in or a `<id>.theme` file in the themes folder).
    pub theme: String,
    /// Global hotkey that shows/hides the popup, e.g. `ctrl+alt+p`.
    pub hotkey: String,
    /// Vault file opened at startup (falls back to the default vault).
    pub last_vault: Option<PathBuf>,
    /// Always show the small floating "paddy" button (it also appears by itself
    /// whenever there is no system tray to click, e.g. under WSL).
    pub mini_button: bool,
    pub window_bar: WindowBar,
    /// Font family for the whole UI (a monospace font suits the look).
    pub font: String,
    /// Font size in px, kept within `MIN_FONT_SIZE..=MAX_FONT_SIZE`.
    pub font_size: u8,
    pub density: Density,
    /// Short event-driven animations (save, open). Never runs while idle.
    pub animations: bool,
    /// Seconds after which a copied secret field is removed from the clipboard (0 = never).
    pub clip_clear_secs: u32,
    /// Key binding overrides from `key.<action> = chords` lines (see `keymap`).
    pub keys: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: crate::theme::DEFAULT_THEME.into(),
            hotkey: "ctrl+alt+p".into(),
            last_vault: None,
            mini_button: false,
            window_bar: WindowBar::Auto,
            font: DEFAULT_FONT.into(),
            font_size: DEFAULT_FONT_SIZE,
            density: Density::Normal,
            animations: true,
            clip_clear_secs: 30,
            keys: BTreeMap::new(),
        }
    }
}

impl Config {
    /// `auto`, `builtin` or `native`, as stored in the file.
    pub fn window_bar_str(&self) -> &'static str {
        self.window_bar.as_str()
    }

    /// Read the config; a missing or unreadable file, and unknown or invalid
    /// lines, silently fall back to defaults (a broken config must not block startup).
    pub fn load(path: &Path) -> Self {
        let mut cfg = Self::default();
        let Ok(text) = fs::read_to_string(path) else { return cfg };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            if let Some(action) = k.strip_prefix("key.") {
                // empty value is meaningful: it unbinds the action
                cfg.keys.insert(action.trim().to_string(), v.to_string());
                continue;
            }
            match k {
                "theme" if crate::theme::valid_id(v) => cfg.theme = v.to_string(),
                "hotkey" if !v.is_empty() => cfg.hotkey = v.to_string(),
                "last_vault" if !v.is_empty() => cfg.last_vault = Some(PathBuf::from(v)),
                "mini_button" => cfg.mini_button = v == "true",
                "window_bar" => cfg.window_bar = WindowBar::parse(v).unwrap_or(cfg.window_bar),
                "font" if !v.is_empty() => cfg.font = v.to_string(),
                "font_size" => {
                    if let Ok(n) = v.parse::<u8>() {
                        cfg.font_size = n.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
                    }
                }
                "density" => cfg.density = Density::parse(v).unwrap_or(cfg.density),
                "animations" => cfg.animations = v != "false",
                "clip_clear_secs" => {
                    if let Ok(n) = v.parse::<u32>() {
                        cfg.clip_clear_secs = n.min(600);
                    }
                }
                _ => {}
            }
        }
        cfg
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        // One value per line: drop newlines and other control characters so a value
        // (say a vault path with a newline in its name) can never inject extra settings.
        let clean = |s: &str| -> String { s.chars().filter(|c| !c.is_control()).collect() };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            crate::fsutil::ensure_private_dir(parent)?;
        }
        let mut text = format!(
            "theme = {}\nhotkey = {}\nmini_button = {}\nwindow_bar = {}\nfont = {}\nfont_size = {}\ndensity = {}\nanimations = {}\nclip_clear_secs = {}\n",
            self.theme,
            clean(&self.hotkey),
            self.mini_button,
            self.window_bar.as_str(),
            clean(&self.font),
            self.font_size,
            self.density.as_str(),
            self.animations,
            self.clip_clear_secs
        );
        for (action, chords) in &self.keys {
            text.push_str(&format!("key.{} = {}\n", clean(action), clean(chords)));
        }
        if let Some(v) = &self.last_vault {
            text.push_str(&format!("last_vault = {}\n", clean(&v.display().to_string())));
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        fs::write(&tmp, text)?;
        crate::fsutil::make_private_file(&tmp)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Where paddy keeps its files. `$PADDY_HOME` overrides everything (handy for
/// tests and portable use); otherwise XDG dirs with `~/.config` / `~/.local/share` fallbacks.
pub struct Paths {
    pub config_file: PathBuf,
    pub vaults_dir: PathBuf,
    /// `<id>.theme` files (imported or hand-made).
    pub themes_dir: PathBuf,
    /// Downloaded font files.
    pub fonts_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Self {
        if let Some(home) = std::env::var_os("PADDY_HOME") {
            let home = PathBuf::from(home);
            return Self::under(&home.join("config"), &home.join("data"));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| ".".into());
        let xdg = |var: &str, fallback: &str| {
            std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(fallback))
        };
        Self::under(&xdg("XDG_CONFIG_HOME", ".config"), &xdg("XDG_DATA_HOME", ".local/share"))
    }

    fn under(config: &Path, data: &Path) -> Self {
        Self {
            config_file: config.join("paddy/config"),
            vaults_dir: data.join("paddy/vaults"),
            themes_dir: data.join("paddy/themes"),
            fonts_dir: data.join("paddy/fonts"),
        }
    }

    pub fn default_vault(&self) -> PathBuf {
        self.vaults_dir.join("default.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config");
        let cfg = Config {
            theme: "nord".into(),
            hotkey: "super+space".into(),
            last_vault: Some("/x/y z/lab.db".into()),
            mini_button: true,
            window_bar: WindowBar::Builtin,
            font: "Fira Code".into(),
            font_size: 17,
            density: Density::Roomy,
            animations: false,
            clip_clear_secs: 60,
            keys: BTreeMap::from([
                ("save".to_string(), "ctrl+shift+s, f2".to_string()),
                ("zoom_in".to_string(), String::new()),
            ]),
        };
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path), cfg);
        assert!(!dir.path().join("sub/config.tmp").exists());
    }

    #[test]
    fn missing_and_garbage_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(&dir.path().join("nope")), Config::default());
        let path = dir.path().join("config");
        fs::write(&path, "theme = Not Valid!\nnonsense\nhotkey =\n= x\nunknown = 1\n").unwrap();
        assert_eq!(Config::load(&path), Config::default());
        fs::write(&path, "  theme =  light \n").unwrap();
        assert_eq!(Config::load(&path).theme, "light");
    }

    #[test]
    fn clipboard_clear_setting_is_clamped_and_optional() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        assert_eq!(Config::default().clip_clear_secs, 30, "on by default");
        fs::write(&path, "clip_clear_secs = 0\n").unwrap();
        assert_eq!(Config::load(&path).clip_clear_secs, 0);
        fs::write(&path, "clip_clear_secs = 99999\n").unwrap();
        assert_eq!(Config::load(&path).clip_clear_secs, 600);
        fs::write(&path, "clip_clear_secs = soon\n").unwrap();
        assert_eq!(Config::load(&path).clip_clear_secs, 30);
    }

    #[test]
    fn a_value_with_newlines_cannot_inject_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let mut c = Config {
            last_vault: Some("/tmp/x\nkey.save = ctrl+q\nfont = Evil".into()),
            font: "Mono\r\nkey.help = ctrl+h".into(),
            ..Config::default()
        };
        c.keys.insert("theme".into(), "ctrl+t\nkey.save = f9".into());
        c.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("key.save")).count(), 0, "{text}");
        let allowed = [
            "theme",
            "hotkey",
            "mini_button",
            "window_bar",
            "font",
            "font_size",
            "density",
            "animations",
            "clip_clear_secs",
            "last_vault",
            "key.theme",
        ];
        for line in text.lines() {
            let key = line.split('=').next().unwrap().trim();
            assert!(allowed.contains(&key), "unexpected line {line:?} in {text}");
        }
        let back = Config::load(&path);
        assert!(!back.keys.contains_key("save") && !back.keys.contains_key("help"), "{:?}", back.keys);
        assert_eq!(text.lines().filter(|l| l.starts_with("font =")).count(), 1);
    }

    #[test]
    fn key_overrides_survive_the_file_including_unbound_and_equals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        fs::write(&path, "key.zoom_in = ctrl+=, ctrl++\nkey.help =\nkey.save=f2\ntheme = light\n").unwrap();
        let c = Config::load(&path);
        assert_eq!(c.keys["zoom_in"], "ctrl+=, ctrl++");
        assert_eq!(c.keys["help"], "");
        assert_eq!(c.keys["save"], "f2");
        assert_eq!(c.theme, "light");
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).keys, c.keys);
    }

    #[test]
    fn font_settings_load_clamped_and_validated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        fs::write(&path, "font = Hack\nfont_size = 99\ndensity = roomy\nanimations = false\n").unwrap();
        let c = Config::load(&path);
        assert_eq!(
            (c.font.as_str(), c.font_size, c.density, c.animations),
            ("Hack", MAX_FONT_SIZE, Density::Roomy, false)
        );
        fs::write(&path, "font_size = 2\ndensity = huge\nfont =\nanimations = maybe\n").unwrap();
        let c = Config::load(&path);
        assert_eq!(
            (c.font.as_str(), c.font_size, c.density, c.animations),
            (DEFAULT_FONT, MIN_FONT_SIZE, Density::Normal, true)
        );
        fs::write(&path, "font_size = big\n").unwrap();
        assert_eq!(Config::load(&path).font_size, DEFAULT_FONT_SIZE);
        assert!(Density::Compact.row_scale() < Density::Normal.row_scale());
        assert!(Density::Normal.row_scale() < Density::Roomy.row_scale());
        assert!(Density::Compact.gap_scale() < Density::Normal.gap_scale());
        assert!(Density::Normal.gap_scale() < Density::Roomy.gap_scale());
    }

    #[test]
    fn window_bar_modes() {
        assert!(WindowBar::Builtin.use_builtin());
        assert!(!WindowBar::Native.use_builtin());
        assert_eq!(WindowBar::Auto.use_builtin(), running_under_wsl());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        fs::write(&path, "window_bar = native\n").unwrap();
        assert_eq!(Config::load(&path).window_bar, WindowBar::Native);
        fs::write(&path, "window_bar = weird\n").unwrap();
        assert_eq!(Config::load(&path).window_bar, WindowBar::Auto);
    }
}
