//! Living in the background like a chat app: close to tray, start hidden at
//! login, start on login, and handing later launches to the running paddy.

use slint::{CloseRequestResponse, ComponentHandle};

use crate::app::App;

impl App {
    /// The main window's close button (OS or built-in bar). With close-to-tray on,
    /// the window only hides and paddy keeps running; otherwise save and quit.
    pub fn close_requested(&self) -> CloseRequestResponse {
        if self.config.borrow().close_to_tray {
            self.to_background();
        } else {
            self.save_on_exit();
            let _ = slint::quit_event_loop();
        }
        CloseRequestResponse::HideWindow
    }

    /// Hide the main window but keep the app (tray, hotkey, quick list) running.
    pub fn hide_main(&self) {
        self.to_background();
        if let Some(ui) = self.ui.upgrade() {
            let _ = ui.hide();
        }
    }

    /// Save, and make sure there is still a way back once the window is gone.
    fn to_background(&self) {
        if self.vault.borrow().is_dirty() {
            self.save();
        }
        if !self.tray_ok() && !self.mini_visible() {
            // No tray icon to click: the floating launcher is the way back.
            self.set_mini_visible(true);
        }
    }

    /// True when a tray icon is up (the desktop side may not be started, e.g. in tests).
    pub fn tray_ok(&self) -> bool {
        self.desktop.borrow().as_ref().is_some_and(|d| d.tray_ok)
    }

    /// Show, raise and focus the main window.
    pub fn show_main(&self) {
        if let Some(ui) = self.ui.upgrade() {
            let _ = ui.show();
            crate::windowctl::focus(ui.window());
            self.focus_list();
        }
    }

    /// Route "show yourself" requests from later paddy launches into the tray's event queue.
    /// Call after `start_desktop`.
    #[cfg(unix)]
    pub fn listen_for_launches(&self, primary: &mut crate::instance::Primary) {
        if let Some(d) = self.desktop.borrow().as_ref() {
            primary.listen(d.sender());
        }
    }

    /// Quit for real (tray "Quit", the settings button): save and end the event loop.
    pub fn quit(&self) {
        self.save_on_exit();
        let _ = slint::quit_event_loop();
    }
}
