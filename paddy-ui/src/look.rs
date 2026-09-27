//! Look and feel: font family and size, row density, and the logo animation.
//! Everything is applied to all three windows through their `Palette` globals.

use std::rc::Rc;
use std::time::Duration;

use paddy_core::{Density, DEFAULT_FONT_SIZE, MAX_FONT_SIZE, MIN_FONT_SIZE};
use slint::{ComponentHandle, SharedString, TimerMode};

use crate::app::App;
use crate::logo;
use crate::{AppState, Palette, PickRow};

/// Fixed fallback list, used when fontconfig's `fc-list` is not installed.
const COMMON_MONO: &[&str] = &[
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
    "Ubuntu Mono",
    "Source Code Pro",
    "JetBrains Mono",
    "Fira Code",
    "Hack",
    "Cascadia Mono",
    "Consolas",
    "Courier New",
];

/// Parse `fc-list :spacing=mono family` output: one line per face, families
/// comma-separated (aliases), possibly with `\-` escapes. Deduped and sorted.
pub fn parse_fc_list(out: &str) -> Vec<String> {
    let mut fonts: Vec<String> = out
        .lines()
        .filter_map(|l| l.split(',').next())
        .map(|f| f.replace("\\-", "-").trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    fonts.sort_by_key(|f| f.to_lowercase());
    fonts.dedup_by_key(|f| f.to_lowercase());
    fonts
}

/// Installed monospace fonts (ask fontconfig; fall back to a common list).
pub fn installed_fonts() -> Vec<String> {
    // Prefer the system copy by absolute path, so a `fc-list` planted earlier in PATH is not run.
    let fc = ["/usr/bin/fc-list", "/bin/fc-list", "/usr/local/bin/fc-list"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap_or("fc-list");
    let found = std::process::Command::new(fc)
        .args([":spacing=mono", "family"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_fc_list(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    if found.is_empty() {
        COMMON_MONO.iter().map(|s| s.to_string()).collect()
    } else {
        found
    }
}

pub fn clamp_size(px: i32) -> u8 {
    px.clamp(MIN_FONT_SIZE as i32, MAX_FONT_SIZE as i32) as u8
}

impl App {
    pub(crate) fn wire_look(self: &Rc<Self>, st: &AppState<'_>) {
        st.set_fp_rows(self.fp_rows.clone().into());
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        bind!(on_bump_font_size, bump_font_size, d);
        bind!(on_reset_font_size, reset_font_size);
        bind!(on_set_density, set_density, d);
        bind!(on_toggle_anim, toggle_anim);
        bind!(on_set_clip_secs, set_clip_secs, n);
        bind!(on_open_fonts, open_fonts);
        bind!(on_close_fonts, close_fonts);
        bind!(on_fp_edited, rebuild_fonts);
        bind!(on_fp_select, fp_select, i);
        bind!(on_fp_open, fp_open);
    }

    /// Push family, size and density from the config into every window.
    pub(crate) fn apply_look(&self) {
        let (font, size, density, anim, clip) = {
            let c = self.config.borrow();
            (c.font.clone(), c.font_size, c.density, c.animations, c.clip_clear_secs)
        };
        self.set_palette_font(&font);
        let apply = |p: &Palette<'_>| {
            p.set_fs(size as f32);
            p.set_row_scale(density.row_scale());
            p.set_gap_scale(density.gap_scale());
        };
        if let Some(ui) = self.ui.upgrade() {
            apply(&ui.global::<Palette>());
        }
        apply(&self.quick.global::<Palette>());
        apply(&self.mini.global::<Palette>());
        self.with_state(|s| {
            s.set_font_name(font.into());
            s.set_font_size_px(size as i32);
            s.set_density_mode(density.as_str().into());
            s.set_anim_on(anim);
            s.set_clip_secs(clip as i32);
        });
    }

    fn set_palette_font(&self, font: &str) {
        let f: SharedString = font.into();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<Palette>().set_font(f.clone());
        }
        self.quick.global::<Palette>().set_font(f.clone());
        self.mini.global::<Palette>().set_font(f);
    }

    pub(crate) fn save_config(&self) {
        let res = self.config.borrow().save(&self.paths.config_file);
        self.report(res);
    }

    pub(crate) fn bump_font_size(&self, delta: i32) {
        let cur = self.config.borrow().font_size as i32;
        self.config.borrow_mut().font_size = clamp_size(cur + delta);
        self.apply_look();
        self.save_config();
    }

    pub(crate) fn reset_font_size(&self) {
        self.config.borrow_mut().font_size = DEFAULT_FONT_SIZE;
        self.apply_look();
        self.save_config();
    }

    fn set_density(&self, name: SharedString) {
        if let Some(d) = Density::parse(name.as_str()) {
            self.config.borrow_mut().density = d;
            self.apply_look();
            self.save_config();
        }
    }

    fn set_clip_secs(&self, n: i32) {
        self.config.borrow_mut().clip_clear_secs = n.clamp(0, 600) as u32;
        self.apply_look();
        self.save_config();
    }

    fn toggle_anim(&self) {
        let on = {
            let mut c = self.config.borrow_mut();
            c.animations = !c.animations;
            c.animations
        };
        self.apply_look();
        self.save_config();
        if on {
            self.play_logo();
        }
    }

    // ---- font picker (live preview; Esc puts the saved font back) ----

    fn open_fonts(&self) {
        let mut all = installed_fonts();
        all.extend(crate::fonts::downloaded_families(&self.paths.fonts_dir));
        all.sort_by_key(|f| f.to_lowercase());
        all.dedup_by_key(|f| f.to_lowercase());
        *self.fonts.borrow_mut() = all;
        self.with_state(|s| {
            s.set_fp_query("".into());
            s.set_show_fonts(true);
        });
        self.rebuild_fonts();
        // start on the current font
        let cur = self.config.borrow().font.to_lowercase();
        let idx = self.fp_names.borrow().iter().position(|f| f.to_lowercase() == cur);
        if let Some(i) = idx {
            self.fp_select(i as i32);
        }
    }

    fn rebuild_fonts(&self) {
        let q = self.with_state(|s| s.get_fp_query().trim().to_lowercase());
        let cur = self.config.borrow().font.to_lowercase();
        let names: Vec<String> =
            self.fonts.borrow().iter().filter(|f| f.to_lowercase().contains(&q)).cloned().collect();
        let rows: Vec<PickRow> = names
            .iter()
            .map(|n| PickRow {
                primary: n.as_str().into(),
                // a sample line: it is drawn in whichever font is being previewed
                secondary: if n.to_lowercase() == cur {
                    "10.10.10.5  ssh admin@host -p 22    ◂ current".into()
                } else {
                    "10.10.10.5  ssh admin@host -p 22".into()
                },
            })
            .collect();
        let sel = if rows.is_empty() { -1 } else { 0 };
        self.fp_rows.set_vec(rows);
        *self.fp_names.borrow_mut() = names;
        self.with_state(|s| s.set_fp_selected(sel));
        if sel == 0 {
            self.fp_select(0);
        }
    }

    fn fp_select(&self, i: i32) {
        let name = usize::try_from(i).ok().and_then(|i| self.fp_names.borrow().get(i).cloned());
        if let Some(n) = name {
            self.with_state(|s| s.set_fp_selected(i));
            self.set_palette_font(&n); // preview only, nothing saved yet
        }
    }

    fn close_fonts(&self) {
        let saved = self.config.borrow().font.clone();
        self.set_palette_font(&saved);
        self.with_state(|s| s.set_show_fonts(false));
        self.focus_list();
    }

    fn fp_open(&self) {
        let idx = self.with_state(|s| s.get_fp_selected());
        let name = usize::try_from(idx).ok().and_then(|i| self.fp_names.borrow().get(i).cloned());
        let Some(name) = name else { return };
        self.config.borrow_mut().font = name;
        self.save_config();
        self.apply_look();
        self.with_state(|s| s.set_show_fonts(false));
        self.focus_list();
    }

    // ---- logo animation: event-driven, stops itself, zero cost while idle ----

    pub(crate) fn set_logo_frame(&self, frame: usize) {
        let (big, mini) = (logo::big_path(frame), logo::mini_path(frame));
        self.with_state(|s| {
            s.set_logo(big.into());
            s.set_logo_mini(mini.into());
        });
    }

    /// Play the "writing" animation once (save, open, launch). No-op if animations are off.
    pub fn play_logo(&self) {
        if !self.config.borrow().animations {
            return;
        }
        let Some(me) = self.me.borrow().upgrade() else { return };
        self.logo_frame.set(1);
        self.set_logo_frame(1);
        let app = Rc::downgrade(&me);
        self.logo_timer.start(TimerMode::Repeated, Duration::from_millis(90), move || {
            let Some(app) = app.upgrade() else { return };
            let next = app.logo_frame.get() + 1;
            if next >= logo::FRAMES {
                app.logo_timer.stop();
                app.logo_frame.set(0);
                app.set_logo_frame(0);
            } else {
                app.logo_frame.set(next);
                app.set_logo_frame(next);
            }
        });
    }

    #[doc(hidden)]
    pub fn quick_palette_bg(&self) -> slint::Color {
        self.quick.global::<Palette>().get_bg()
    }

    #[doc(hidden)]
    pub fn quick_palette_fs(&self) -> f32 {
        self.quick.global::<Palette>().get_fs()
    }

    pub fn logo_animating(&self) -> bool {
        self.logo_timer.running()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fc_list_output() {
        let out = "DejaVu Sans Mono,DejaVu Sans Mono Bold\nFira Code\nDejaVu Sans Mono\nnoto sans mono\n\n  \nJetBrains Mono\\-Foo\n";
        assert_eq!(parse_fc_list(out), ["DejaVu Sans Mono", "Fira Code", "JetBrains Mono-Foo", "noto sans mono"]);
        assert!(parse_fc_list("").is_empty());
    }

    #[test]
    fn size_is_clamped() {
        assert_eq!(clamp_size(-5), MIN_FONT_SIZE);
        assert_eq!(clamp_size(13), 13);
        assert_eq!(clamp_size(500), MAX_FONT_SIZE);
    }

    #[test]
    fn fallback_font_list_is_not_empty() {
        assert!(!installed_fonts().is_empty());
    }
}
