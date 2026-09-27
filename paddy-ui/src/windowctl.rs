//! Moving, resizing and closing windows for the built-in title bar.
//! Slint hands us pointer deltas in logical pixels while a bar/grip is dragged.

use std::rc::Rc;

use slint::platform::WindowEvent;
use slint::winit_030::{winit, WinitWindowAccessor};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, Window};

use crate::app::App;
use crate::{AppState, MainWindow, PopupState};

/// Smallest size the main window may be dragged to.
const MIN_MAIN: (i32, i32) = (780, 460);

/// Let the window manager move the window (true), or report that we must do it ourselves.
pub fn native_drag(w: &Window) -> bool {
    w.with_winit_window(|ww| ww.drag_window().is_ok()).unwrap_or(false)
}

pub fn native_resize(w: &Window) -> bool {
    w.with_winit_window(|ww| ww.drag_resize_window(winit::window::ResizeDirection::SouthEast).is_ok()).unwrap_or(false)
}

/// Re-assert always-on-top once the window is on screen. X11 window managers (xfwm4
/// among them) ignore the request when it's made before the window is mapped, which
/// is when the toolkit makes it, so the quick list could open behind other windows.
pub fn keep_on_top(w: &Window) {
    let _ = w.with_winit_window(|ww| {
        ww.set_window_level(winit::window::WindowLevel::Normal);
        ww.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
    });
}

/// Ask for keyboard focus (X11 WMs with focus-stealing prevention otherwise leave it elsewhere).
pub fn focus(w: &Window) {
    let _ = w.with_winit_window(|ww| ww.focus_window());
}

pub fn drag(w: &Window, dx: f32, dy: f32) {
    let s = w.scale_factor();
    let p = w.position();
    w.set_position(PhysicalPosition::new(p.x + (dx * s).round() as i32, p.y + (dy * s).round() as i32));
}

pub fn resized(current: (u32, u32), dx: f32, dy: f32, scale: f32, min: (i32, i32)) -> (u32, u32) {
    let w = (current.0 as i32 + (dx * scale).round() as i32).max((min.0 as f32 * scale) as i32);
    let h = (current.1 as i32 + (dy * scale).round() as i32).max((min.1 as f32 * scale) as i32);
    (w as u32, h as u32)
}

fn resize(w: &Window, dx: f32, dy: f32) {
    let cur = w.size();
    let (nw, nh) = resized((cur.width, cur.height), dx, dy, w.scale_factor(), MIN_MAIN);
    w.set_size(PhysicalSize::new(nw, nh));
}

impl App {
    /// Connect the built-in title bar controls of all three windows.
    pub(crate) fn wire_windows(self: &Rc<Self>, ui: &MainWindow) {
        let weak = ui.as_weak();
        ui.on_drag_window({
            let w = weak.clone();
            move |dx, dy| {
                if let Some(ui) = w.upgrade() {
                    drag(ui.window(), dx, dy);
                }
            }
        });
        ui.on_resize_window({
            let w = weak.clone();
            move |dx, dy| {
                if let Some(ui) = w.upgrade() {
                    resize(ui.window(), dx, dy);
                }
            }
        });
        ui.on_start_drag({
            let w = weak.clone();
            move || w.upgrade().is_some_and(|ui| native_drag(ui.window()))
        });
        ui.on_start_resize({
            let w = weak.clone();
            move || w.upgrade().is_some_and(|ui| native_resize(ui.window()))
        });
        ui.on_minimize_window({
            let w = weak.clone();
            move || {
                if let Some(ui) = w.upgrade() {
                    ui.window().set_minimized(true);
                }
            }
        });
        ui.on_toggle_maximize({
            let w = weak.clone();
            move || {
                if let Some(ui) = w.upgrade() {
                    let win = ui.window();
                    win.set_maximized(!win.is_maximized());
                }
            }
        });
        // Same path as the OS close button, so saving on exit still runs.
        ui.on_close_window({
            let w = weak;
            move || {
                if let Some(ui) = w.upgrade() {
                    ui.window().dispatch_event(WindowEvent::CloseRequested);
                }
            }
        });

        let q = self.quick.as_weak();
        self.quick.on_drag_window({
            let q = q.clone();
            move |dx, dy| {
                if let Some(w) = q.upgrade() {
                    drag(w.window(), dx, dy);
                }
            }
        });
        self.quick.on_start_drag({
            let q = q.clone();
            move || q.upgrade().is_some_and(|w| native_drag(w.window()))
        });
        let app = self.clone();
        self.quick.on_close_window(move || app.toggle_popup());

        let m = self.mini.as_weak();
        self.mini.on_start_drag({
            let m = m.clone();
            move || m.upgrade().is_some_and(|w| native_drag(w.window()))
        });
        self.mini.on_drag_window(move |dx, dy| {
            if let Some(w) = m.upgrade() {
                drag(w.window(), dx, dy);
            }
        });
    }

    /// Turn the built-in title bar on or off for every window.
    pub(crate) fn apply_window_bar(&self) {
        let builtin = self.config.borrow().window_bar.use_builtin();
        let mode = self.config.borrow().window_bar_str();
        self.with_state(|s: &AppState<'_>| {
            s.set_builtin_bar(builtin);
            s.set_window_bar_mode(mode.into());
        });
        self.quick.global::<PopupState>().set_builtin_bar(builtin);
        self.mini.set_builtin_bar(builtin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_clamps_to_minimum() {
        assert_eq!(resized((1000, 640), 50.0, -40.0, 1.0, MIN_MAIN), (1050, 600));
        assert_eq!(resized((800, 470), -500.0, -500.0, 1.0, MIN_MAIN), (780, 460));
        assert_eq!(resized((1600, 1000), 10.0, 10.0, 2.0, MIN_MAIN), (1620, 1020));
    }
}
