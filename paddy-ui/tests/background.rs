//! Background behavior on the real UI (headless): the settings toggles, close to
//! tray, and the ways back to the main window. Set `PADDY_SHOTS=<dir>` for PNGs.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use paddy_core::{create_vault, Config, Paths};
use paddy_ui::{app::App, AppState, MainWindow};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, PhysicalSize};

#[derive(Default)]
struct Headless(Rc<RefCell<Vec<Rc<MinimalSoftwareWindow>>>>);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let w = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        w.set_size(PhysicalSize::new(1000, 640));
        self.0.borrow_mut().push(w.clone());
        Ok(w)
    }
}

fn shot(win: &MinimalSoftwareWindow, name: &str) {
    let Some(dir) = std::env::var_os("PADDY_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&dir).unwrap();
    let size = win.size();
    let (w, h) = (size.width, size.height);
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    win.request_redraw();
    win.draw_if_needed(|r| {
        r.render(&mut buf, w as usize);
    });
    let rgb: Vec<u8> = buf.iter().flat_map(|p| [p.red, p.green, p.blue]).collect();
    let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgb).unwrap();
}

#[test]
fn background_mode() {
    let platform = Headless::default();
    let windows = platform.0.clone();
    slint::platform::set_platform(Box::new(platform)).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data/paddy");
    let paths = Paths {
        config_file: tmp.path().join("config/paddy/config"),
        vaults_dir: data.join("vaults"),
        themes_dir: data.join("themes"),
        fonts_dir: data.join("fonts"),
    };
    let cfg_path = paths.config_file.clone();
    let autostart = paths.autostart_file();
    assert!(autostart.starts_with(tmp.path()), "tests never touch the real ~/.config/autostart");
    let vault = create_vault(&paths.vaults_dir, "lab").unwrap();

    let ui = MainWindow::new().unwrap();
    let win = windows.borrow()[0].clone();
    let app = App::new(&ui, vault, Config::default(), paths);
    ui.window().on_close_requested({
        let app = app.clone();
        move || app.close_requested()
    });
    ui.show().unwrap();
    let st = ui.global::<AppState>();

    // ---- settings: defaults, then each toggle is saved ----
    st.invoke_open_settings();
    assert!(!st.get_autostart_on() && st.get_start_hidden_on() && st.get_close_to_tray_on());
    shot(&win, "30-settings-background");

    st.invoke_toggle_autostart();
    assert!(st.get_autostart_on(), "{}", st.get_settings_note());
    let text = std::fs::read_to_string(&autostart).unwrap();
    let exe = std::env::current_exe().unwrap();
    assert!(text.contains(&format!("Exec={} --autostart", exe.display())), "{text}");
    assert!(Config::load(&cfg_path).autostart);
    assert!(paddy_core::autostart_installed(&autostart));
    st.invoke_toggle_autostart();
    assert!(!st.get_autostart_on() && !autostart.exists());
    assert!(!Config::load(&cfg_path).autostart);

    st.invoke_toggle_start_hidden();
    assert!(!st.get_start_hidden_on() && !Config::load(&cfg_path).start_hidden);
    st.invoke_toggle_start_hidden();
    assert!(Config::load(&cfg_path).start_hidden);
    st.invoke_close_settings();

    // ---- close to tray: the window hides, edits are saved, a way back exists ----
    st.invoke_add_entry();
    assert!(st.get_dirty(), "an unsaved edit");
    assert!(!app.mini_visible() && !app.tray_ok());
    win.dispatch_event(WindowEvent::CloseRequested);
    assert!(!ui.window().is_visible(), "closed to the background");
    assert!(!st.get_dirty(), "saved on the way out");
    assert!(app.mini_visible(), "no tray here, so the floating launcher is the way back");

    // the quick list's "open paddy" brings the window back
    app.quick_state().invoke_open_main();
    assert!(ui.window().is_visible());
    // and so does the tray's "Open paddy" / a second launch (ShowMain)
    app.hide_main();
    assert!(!ui.window().is_visible());
    app.show_main();
    assert!(ui.window().is_visible());

    // ---- close-to-tray off: closing quits like before ----
    st.invoke_open_settings();
    st.invoke_toggle_close_to_tray();
    assert!(!st.get_close_to_tray_on() && !Config::load(&cfg_path).close_to_tray);
    assert!(st.get_settings_note().contains("quits"));
    shot(&win, "31-settings-background-off");
    win.dispatch_event(WindowEvent::CloseRequested);
    assert!(!ui.window().is_visible());
}
