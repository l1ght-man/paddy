//! Themes in the UI: loading (built-in + user files), applying to every window,
//! the live-preview picker, and saving imported themes.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;

use paddy_core::{builtin_themes, find_theme, Rgb, Theme, DEFAULT_LIGHT, DEFAULT_THEME, MAX_THEME_BYTES};
use slint::{Color, ComponentHandle};

use crate::app::App;
use crate::{AppState, Palette, PickRow};

/// Most theme files we will look at in the themes folder.
const MAX_USER_THEMES: usize = 100;

fn color(c: Rgb) -> Color {
    Color::from_rgb_u8(c.0, c.1, c.2)
}

/// Read `*.theme` files from `dir`. Only regular files (no symlinks), size-capped,
/// whose file name matches the theme id and that don't reuse a built-in id.
/// Returns the good ones and a warning per rejected file.
pub fn load_user_themes(dir: &Path) -> (Vec<Theme>, Vec<String>) {
    let (mut themes, mut warnings) = (Vec::new(), Vec::new());
    let Ok(rd) = fs::read_dir(dir) else { return (themes, warnings) };
    let builtin_ids: Vec<String> = builtin_themes().into_iter().map(|t| t.id).collect();
    let mut paths: Vec<_> =
        rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "theme")).collect();
    paths.sort();
    for path in paths.into_iter().take(MAX_USER_THEMES) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let read = || -> Result<String, String> {
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err("not a regular file".into());
            }
            let mut text = String::new();
            fs::File::open(&path)
                .and_then(|f| f.take(MAX_THEME_BYTES as u64 + 1).read_to_string(&mut text))
                .map_err(|e| e.to_string())?;
            Ok(text)
        };
        match read().and_then(|t| Theme::parse(&t).map_err(|e| e.to_string())) {
            Ok(t) if path.file_stem().is_some_and(|s| s == t.id.as_str()) && !builtin_ids.contains(&t.id) => {
                themes.push(t)
            }
            Ok(t) if builtin_ids.contains(&t.id) => {
                warnings.push(format!("{name}: id \"{}\" is a built-in theme; pick another id", t.id))
            }
            Ok(t) => warnings.push(format!("{name}: file name must be {}.theme", t.id)),
            Err(e) => warnings.push(format!("{name}: {e}")),
        }
    }
    (themes, warnings)
}

/// Save an imported theme as `<id>.theme` (owner-only, atomic). Built-in ids are refused.
pub fn save_theme(dir: &Path, theme: &Theme) -> Result<(), String> {
    if builtin_themes().iter().any(|b| b.id == theme.id) {
        return Err(format!("\"{}\" is a built-in theme id; the theme needs a different id", theme.id));
    }
    paddy_core::ensure_private_dir(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.theme", theme.id));
    let tmp = dir.join(format!("{}.theme.part", theme.id));
    fs::write(&tmp, theme.to_text()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

impl App {
    pub(crate) fn wire_themes(self: &Rc<Self>, st: &AppState<'_>) {
        st.set_tp_rows(self.tp_rows.clone().into());
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        bind!(on_open_themes, open_themes);
        bind!(on_close_themes, close_themes);
        bind!(on_tp_edited, rebuild_themes);
        bind!(on_tp_select, tp_select, i);
        bind!(on_tp_open, tp_open);
        bind!(on_toggle_theme, toggle_theme);
        bind!(on_set_theme, set_theme, dark);
    }

    /// (Re)build the theme list: built-ins first, then the user's files.
    pub(crate) fn load_themes(&self) {
        let mut all = builtin_themes();
        let (user, warnings) = load_user_themes(&self.paths.themes_dir);
        all.extend(user);
        *self.themes.borrow_mut() = all;
        if !warnings.is_empty() {
            self.set_status(&format!("themes: {}", warnings.join("; ")), true);
        }
    }

    fn theme_by_id(&self, id: &str) -> Theme {
        let all = self.themes.borrow();
        find_theme(id, &[&all])
            .or_else(|| find_theme(DEFAULT_THEME, &[&all]))
            .cloned()
            .expect("built-in default theme exists")
    }

    /// Paint a theme into all three windows (not saved).
    pub(crate) fn paint_theme(&self, t: &Theme) {
        let apply = |p: &Palette<'_>| {
            p.set_dark(t.dark);
            p.set_bg(color(t.bg));
            p.set_panel(color(t.panel));
            p.set_fg(color(t.fg));
            p.set_dim(color(t.dim));
            p.set_line(color(t.line));
            p.set_accent(color(t.accent));
            p.set_sel_bg(color(t.sel));
            p.set_warn(color(t.warn));
        };
        if let Some(ui) = self.ui.upgrade() {
            apply(&ui.global::<Palette>());
        }
        apply(&self.quick.global::<Palette>());
        apply(&self.mini.global::<Palette>());
    }

    /// Show the saved theme (config) everywhere.
    pub(crate) fn apply_theme(&self) {
        let id = self.config.borrow().theme.clone();
        let t = self.theme_by_id(&id);
        self.paint_theme(&t);
        self.with_state(|s| s.set_theme_name(t.name.as_str().into()));
    }

    /// Ctrl+L: flip between the light and dark defaults (custom themes go to the matching default).
    pub fn toggle_theme(&self) {
        let cur = self.theme_by_id(&self.config.borrow().theme.clone());
        self.config.borrow_mut().theme = if cur.dark { DEFAULT_LIGHT } else { DEFAULT_THEME }.to_string();
        self.apply_theme();
        self.save_config();
    }

    pub(crate) fn set_theme(&self, dark: bool) {
        let cur = self.theme_by_id(&self.config.borrow().theme.clone());
        if cur.dark != dark {
            self.toggle_theme();
        }
    }

    // ---- theme picker (live preview; Esc puts the saved theme back) ----

    pub fn open_themes(&self) {
        self.load_themes();
        self.with_state(|s| {
            s.set_tp_query("".into());
            s.set_show_themes(true);
        });
        self.rebuild_themes();
        let cur = self.config.borrow().theme.clone();
        let idx = self.tp_ids.borrow().iter().position(|i| *i == cur);
        if let Some(i) = idx {
            // select *and* preview it, so the highlight and the painted theme agree
            self.tp_select(i as i32);
        }
    }

    fn rebuild_themes(&self) {
        let q = self.with_state(|s| s.get_tp_query().trim().to_lowercase());
        let cur = self.config.borrow().theme.clone();
        let builtin: Vec<String> = builtin_themes().into_iter().map(|t| t.id).collect();
        let shown: Vec<Theme> = self
            .themes
            .borrow()
            .iter()
            .filter(|t| t.name.to_lowercase().contains(&q) || t.id.contains(&q))
            .cloned()
            .collect();
        let rows: Vec<PickRow> = shown
            .iter()
            .map(|t| PickRow {
                primary: t.name.as_str().into(),
                secondary: format!(
                    "{} · {}{}",
                    if t.dark { "dark" } else { "light" },
                    if builtin.contains(&t.id) { "built in" } else { "yours" },
                    if t.id == cur { "   ◂ current" } else { "" }
                )
                .into(),
            })
            .collect();
        let sel = if rows.is_empty() { -1 } else { 0 };
        self.tp_rows.set_vec(rows);
        *self.tp_ids.borrow_mut() = shown.into_iter().map(|t| t.id).collect();
        self.with_state(|s| s.set_tp_selected(sel));
        if sel == 0 {
            self.tp_select(0);
        }
    }

    fn tp_select(&self, i: i32) {
        let id = usize::try_from(i).ok().and_then(|i| self.tp_ids.borrow().get(i).cloned());
        if let Some(id) = id {
            self.with_state(|s| s.set_tp_selected(i));
            let t = self.theme_by_id(&id);
            self.paint_theme(&t); // preview only
        }
    }

    pub(crate) fn close_themes(&self) {
        self.apply_theme(); // restore the saved theme
        self.with_state(|s| s.set_show_themes(false));
        self.focus_list();
    }

    fn tp_open(&self) {
        let idx = self.with_state(|s| s.get_tp_selected());
        let id = usize::try_from(idx).ok().and_then(|i| self.tp_ids.borrow().get(i).cloned());
        let Some(id) = id else { return };
        self.config.borrow_mut().theme = id;
        self.save_config();
        self.apply_theme();
        self.with_state(|s| s.set_show_themes(false));
        self.focus_list();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        fs::write(dir.join(name), text).unwrap();
    }

    fn good(id: &str) -> String {
        builtin_themes()[2]
            .to_text()
            .replace("id = nord", &format!("id = {id}"))
            .replace("name = Nord", &format!("name = {id} theme"))
    }

    #[test]
    fn loads_valid_user_themes_and_reports_bad_ones() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "mine.theme", &good("mine"));
        write(dir.path(), "wrongname.theme", &good("other"));
        write(dir.path(), "dark.theme", &good("dark")); // spoofs a built-in id
        write(dir.path(), "broken.theme", "id = broken\n");
        write(dir.path(), "readme.txt", "ignored");
        write(dir.path(), "huge.theme", &"x".repeat(MAX_THEME_BYTES * 2));
        let (themes, warnings) = load_user_themes(dir.path());
        assert_eq!(themes.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["mine"]);
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert!(warnings.iter().any(|w| w.contains("built-in")));
        assert!(warnings.iter().any(|w| w.contains("file name must be")));
        assert!(load_user_themes(&dir.path().join("missing")).0.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_theme_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), good("linked")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked.theme")).unwrap();
        let (themes, warnings) = load_user_themes(dir.path());
        assert!(themes.is_empty());
        assert!(warnings[0].contains("not a regular file"), "{warnings:?}");
    }

    #[test]
    fn saving_round_trips_and_refuses_builtin_ids() {
        let dir = tempfile::tempdir().unwrap();
        let t = Theme::parse(&good("fresh")).unwrap();
        save_theme(dir.path(), &t).unwrap();
        assert_eq!(load_user_themes(dir.path()).0, vec![t.clone()]);
        assert!(!dir.path().join("fresh.theme.part").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(dir.path().join("fresh.theme")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let mut evil = t;
        evil.id = "light".into();
        assert!(save_theme(dir.path(), &evil).unwrap_err().contains("built-in"));
        assert!(!dir.path().join("light.theme").exists());
    }
}
