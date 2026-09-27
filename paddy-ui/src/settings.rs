//! Settings overlay actions and the floating launcher button.

use std::rc::Rc;

use std::time::Duration;

use paddy_core::WindowBar;
use slint::{ComponentHandle, Model, SharedString, TimerMode};

use crate::app::App;
use crate::desktop::validate_hotkey;
use crate::AppState;

impl App {
    pub(crate) fn wire_settings(self: &Rc<Self>, st: &AppState<'_>) {
        macro_rules! bind {
            ($on:ident, $method:ident $(, $arg:ident)*) => {{
                let app = self.clone();
                st.$on(move |$($arg),*| app.$method($($arg),*));
            }};
        }
        {
            let app = self.clone();
            st.on_open_settings(move || app.open_settings());
        }
        bind!(on_close_settings, close_settings);
        bind!(on_open_help, open_help);
        bind!(on_close_help, close_help);
        bind!(on_apply_hotkey, apply_hotkey);
        bind!(on_toggle_mini, toggle_mini);
        bind!(on_set_window_bar, set_window_bar, mode);
        bind!(on_copy_path, copy_path, p);

        let app = self.clone();
        self.mini.on_clicked(move || app.toggle_popup());
    }

    pub fn open_settings(&self) {
        let hotkey = self.config.borrow().hotkey.clone();
        let (cfg, vaults) = (self.paths.config_file.display().to_string(), self.paths.vaults_dir.display().to_string());
        let mini = self.mini.window().is_visible();
        self.with_state(|s| {
            s.set_settings_hotkey(hotkey.into());
            s.set_settings_note("".into());
            s.set_config_path(cfg.into());
            s.set_vaults_path(vaults.into());
            s.set_mini_on(mini);
            s.set_tab(1);
        });
        self.refresh_diag();
        let Some(me) = self.me.borrow().upgrade() else { return };
        let app = Rc::downgrade(&me);
        self.diag_timer.start(TimerMode::Repeated, Duration::from_secs(2), move || {
            if let Some(app) = app.upgrade() {
                app.refresh_diag();
            }
        });
    }

    pub(crate) fn open_help(&self) {
        self.with_state(|s| s.set_show_help(true));
    }

    pub(crate) fn close_help(&self) {
        self.with_state(|s| s.set_show_help(false));
        self.focus_list();
    }

    pub(crate) fn close_settings(&self) {
        self.diag_timer.stop();
        self.cancel_recording();
        self.with_state(|s| s.set_tab(0));
        self.focus_list();
    }

    fn refresh_diag(&self) {
        let n = self.entries.row_count();
        let text = self.diag.borrow_mut().report(n);
        self.with_state(|s| s.set_diag_text(text.into()));
    }

    fn set_window_bar(&self, mode: SharedString) {
        let bar = match mode.as_str() {
            "builtin" => WindowBar::Builtin,
            "native" => WindowBar::Native,
            _ => WindowBar::Auto,
        };
        self.config.borrow_mut().window_bar = bar;
        let res = self.config.borrow().save(&self.paths.config_file);
        self.report(res);
        self.apply_window_bar();
        self.note("saved. If the window frame looks off, restart paddy.");
    }

    fn note(&self, msg: &str) {
        self.with_state(|s| s.set_settings_note(msg.into()));
    }

    fn apply_hotkey(&self) {
        let spec = self.with_state(|s| s.get_settings_hotkey().trim().to_string());
        if let Err(e) = validate_hotkey(&spec) {
            self.note(&format!("not a valid hotkey ({e}). Example: ctrl+alt+p"));
            return;
        }
        if self.config.borrow().hotkey == spec {
            self.note("that is already the hotkey");
            return;
        }
        self.config.borrow_mut().hotkey = spec.clone();
        let res = self.config.borrow().save(&self.paths.config_file);
        if self.report(res).is_some() {
            self.note(&format!("saved {spec}. Restart paddy to use it."));
        }
    }

    pub fn toggle_mini(&self) {
        let show = !self.mini.window().is_visible();
        self.set_mini_visible(show);
        self.config.borrow_mut().mini_button = show;
        let res = self.config.borrow().save(&self.paths.config_file);
        self.report(res);
    }

    pub fn mini_visible(&self) -> bool {
        self.mini.window().is_visible()
    }

    pub(crate) fn set_mini_visible(&self, show: bool) {
        if show {
            let _ = self.mini.show();
            crate::windowctl::keep_on_top(self.mini.window());
        } else {
            let _ = self.mini.hide();
        }
        self.with_state(|s| s.set_mini_on(show));
    }

    fn copy_path(&self, path: SharedString) {
        if self.copy_text(path.as_str()) {
            self.set_status("copied path", false);
        }
    }
}
