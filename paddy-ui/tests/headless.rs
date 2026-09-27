//! Renders the real UI with Slint's software renderer (no display needed),
//! drives it with key events, and checks state. Set `PADDY_SHOTS=<dir>` to
//! also dump PNG screenshots.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use paddy_core::{create_vault, Config, Density, Entry, Field, Paths, WindowBar};
use paddy_ui::{app::App, AppState, MainWindow, Palette};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::Key;
use slint::platform::{Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, Model, PhysicalSize, SharedString};

/// Hands out one software window per Slint window, in creation order.
#[derive(Default)]
struct Headless(Rc<RefCell<Vec<Rc<MinimalSoftwareWindow>>>>);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let w = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        w.set_size(PhysicalSize::new(W, H));
        self.0.borrow_mut().push(w.clone());
        Ok(w)
    }
}

const W: u32 = 1000;
const H: u32 = 640;

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
    let mut rgb = Vec::with_capacity(buf.len() * 3);
    for p in &buf {
        rgb.extend_from_slice(&[p.red, p.green, p.blue]);
    }
    let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgb).unwrap();
}

/// Run Slint timers until `done()` or a timeout (headless has no event loop).
fn pump_until(mut done: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < end {
        slint::platform::update_timers_and_animations();
        if done() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    false
}

fn key(k: Key) -> String {
    SharedString::from(k).to_string()
}

/// Hash of the rendered frame, to check that a setting visibly changes the screen.
fn frame_hash(win: &MinimalSoftwareWindow) -> u64 {
    use std::hash::{Hash, Hasher};
    let size = win.size();
    let (w, h) = (size.width as usize, size.height as usize);
    let mut buf = vec![PremultipliedRgbaColor::default(); w * h];
    win.request_redraw();
    win.draw_if_needed(|r| {
        r.render(&mut buf, w);
    });
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for p in &buf {
        (p.red, p.green, p.blue).hash(&mut hasher);
    }
    hasher.finish()
}

fn press(win: &MinimalSoftwareWindow, text: &str) {
    let t: SharedString = text.into();
    win.dispatch_event(WindowEvent::KeyPressed { text: t.clone() });
    win.dispatch_event(WindowEvent::KeyReleased { text: t });
}

/// Press `text` while holding the given modifier keys.
fn chord(win: &MinimalSoftwareWindow, mods: &[Key], text: &str) {
    for m in mods {
        win.dispatch_event(WindowEvent::KeyPressed { text: (*m).into() });
    }
    press(win, text);
    for m in mods.iter().rev() {
        win.dispatch_event(WindowEvent::KeyReleased { text: (*m).into() });
    }
}

fn ctrl(win: &MinimalSoftwareWindow, key: &str) {
    win.dispatch_event(WindowEvent::KeyPressed { text: slint::platform::Key::Control.into() });
    press(win, key);
    win.dispatch_event(WindowEvent::KeyReleased { text: slint::platform::Key::Control.into() });
}

fn type_str(win: &MinimalSoftwareWindow, s: &str) {
    for c in s.chars() {
        press(win, &c.to_string());
    }
}

#[test]
fn ui_smoke() {
    let platform = Headless::default();
    let windows = platform.0.clone();
    slint::platform::set_platform(Box::new(platform)).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let vaults_dir = tmp.path().join("vaults");
    let mut other = create_vault(&vaults_dir, "other").unwrap();
    let mut fw = Entry::new("fw01");
    fw.fields = vec![Field::new("host", "192.168.1.1")];
    other.add_entry(&fw).unwrap();
    other.save().unwrap();
    drop(other);
    let mut vault = create_vault(&vaults_dir, "lab").unwrap();
    let mut e = Entry::new("dc01");
    e.tags = vec!["ad".into(), "htb".into()];
    e.notes = "# domain controller\nkerberoast me".into();
    e.fields = vec![
        Field::new("host", "10.10.10.5"),
        Field::secret("password", "hunter2"),
        Field::new("user", "administrator"),
    ];
    vault.add_entry(&e).unwrap();
    vault.add_entry(&Entry::new("web01")).unwrap();

    let ui = MainWindow::new().unwrap();
    let win = windows.borrow()[0].clone();
    let cfg_path = tmp.path().join("config");
    let themes_dir = tmp.path().join("themes");
    let fonts_dir = tmp.path().join("fonts");
    let paths = Paths {
        config_file: cfg_path.clone(),
        vaults_dir: vaults_dir.clone(),
        themes_dir: themes_dir.clone(),
        fonts_dir: fonts_dir.clone(),
    };
    let app = App::new(
        &ui,
        vault,
        Config {
            window_bar: WindowBar::Builtin,
            // a user-edited settings file: zoom reset moved from Ctrl+0 to Ctrl+9
            keys: std::collections::BTreeMap::from([("zoom_reset".to_string(), "ctrl+9".to_string())]),
            ..Config::default()
        },
        paths,
    );
    ui.show().unwrap();
    let st = ui.global::<AppState>();

    assert_eq!(st.get_selected(), 0);
    assert_eq!(st.get_draft_label(), "dc01");
    shot(&win, "1-initial");

    // ---- notes are markdown: edit / preview ----
    assert!(!st.get_notes_preview());
    ctrl(&win, "e");
    assert!(st.get_notes_preview());
    let kinds = |st: &AppState| -> Vec<String> { st.get_md_blocks().iter().map(|b| b.kind.to_string()).collect() };
    assert_eq!(kinds(&st), ["h", "p"], "heading + line from the sample note");
    shot(&win, "16-preview-basic");
    st.set_draft_notes("# Recon\nssh **admin**@10.10.10.5 and `nmap -sC`\n\n- [ ] enum shares\n- [x] scan ports\n  - nested item\n1. first\n> remember this\n---\n```\nsudo nmap -sV 10.10.10.5\n```\nsee https://example.com/docs".into());
    st.invoke_draft_edited();
    st.invoke_toggle_preview();
    st.invoke_toggle_preview();
    assert!(st.get_notes_preview());
    assert_eq!(kinds(&st), ["h", "p", "gap", "li", "li", "li", "li", "quote", "rule", "code", "p"]);
    let markers: Vec<String> =
        st.get_md_blocks().iter().filter(|b| b.kind == "li").map(|b| b.marker.to_string()).collect();
    assert_eq!(markers, ["☐", "☑", "•", "1."]);
    shot(&win, "17-preview-rich");
    // moving to another entry keeps preview on and re-renders
    press(&win, &key(Key::Escape));
    press(&win, &key(Key::DownArrow));
    assert!(st.get_notes_preview());
    assert!(kinds(&st).is_empty(), "web01 has no notes");
    shot(&win, "18-preview-empty");
    press(&win, &key(Key::UpArrow));
    // a link click copies the url (clipboard may be unavailable headless; must not crash)
    st.invoke_md_link("https://example.com/docs".into());
    // the edit button in the editor and Ctrl+E both return to editing; a new entry starts in edit mode
    ctrl(&win, "e");
    assert!(!st.get_notes_preview());
    ctrl(&win, "e");
    assert!(st.get_notes_preview());
    ctrl(&win, "n");
    assert!(!st.get_notes_preview(), "new entries start in edit mode");
    ctrl(&win, "d");
    press(&win, "y");
    press(&win, &key(Key::Escape));

    // ---- keys tab: rebind, conflicts, invalid chords, add, clear, reset ----
    ctrl(&win, "k");
    assert_eq!(st.get_tab(), 2);
    assert_eq!(st.get_key_rows().row_count(), 3 + 16, "3 group headers + 16 actions");
    shot(&win, "19-keys");
    let row = |st: &AppState, id: &str| st.get_key_rows().iter().find(|r| r.id == id).unwrap();
    assert_eq!(row(&st, "theme").chords, "Ctrl+L");
    assert!(row(&st, "theme").is_default);
    // rebind: theme -> Ctrl+Shift+Y
    st.invoke_key_change("theme".into());
    assert_eq!(st.get_recording(), "theme");
    chord(&win, &[Key::Control], "z");
    assert_eq!(st.get_recording(), "", "a free chord binds and stops recording");
    assert_eq!(Config::load(&cfg_path).keys["theme"], "ctrl+z");
    assert_eq!(row(&st, "theme").chords, "Ctrl+Z");
    assert!(!row(&st, "theme").is_default);
    let dark_before = ui.global::<Palette>().get_dark();
    ctrl(&win, "l");
    assert_eq!(ui.global::<Palette>().get_dark(), dark_before, "old chord no longer works");
    ctrl(&win, "z");
    assert_ne!(ui.global::<Palette>().get_dark(), dark_before, "new chord works immediately");
    ctrl(&win, "z");
    // modifiers alone do nothing; conflicts are refused and recording continues
    st.invoke_key_change("save".into());
    win.dispatch_event(WindowEvent::KeyPressed { text: Key::Control.into() });
    assert_eq!(st.get_recording(), "save", "a lone modifier keeps waiting");
    win.dispatch_event(WindowEvent::KeyReleased { text: Key::Control.into() });
    chord(&win, &[Key::Control], "n");
    assert!(st.get_keys_note().contains("already used by"), "{}", st.get_keys_note());
    assert_eq!(st.get_recording(), "save");
    press(&win, "q");
    assert!(st.get_keys_note().contains("break typing"), "{}", st.get_keys_note());
    press(&win, &key(Key::Escape));
    assert_eq!(st.get_recording(), "", "Esc cancels");
    assert!(!Config::load(&cfg_path).keys.contains_key("save"));
    // add a second chord
    st.invoke_key_add("help".into());
    press(&win, &key(Key::F2));
    assert_eq!(Config::load(&cfg_path).keys["help"], "f1, f2");
    press(&win, &key(Key::F2));
    assert!(st.get_show_help(), "the added chord opens help");
    press(&win, &key(Key::F2));
    assert!(!st.get_show_help(), "and toggles it closed again");
    // footer hint follows the keymap
    st.invoke_key_change("new_entry".into());
    chord(&win, &[Key::Control, Key::Shift], "M");
    assert_eq!(Config::load(&cfg_path).keys["new_entry"], "ctrl+shift+m");
    assert_eq!(st.get_hints().row_data(0).unwrap().key, "Ctrl+Shift+M");
    // clear and reset
    st.invoke_key_clear("templates".into());
    assert_eq!(row(&st, "templates").chords, "unbound");
    ctrl(&win, "t");
    assert!(!st.get_show_templates(), "unbound action does nothing");
    st.invoke_key_reset("templates".into());
    assert_eq!(row(&st, "templates").chords, "Ctrl+T");
    assert!(!Config::load(&cfg_path).keys.contains_key("templates"), "defaults aren't written to the file");
    // a plain typing key can't be bound; the row stays as it was
    st.invoke_key_change("find".into());
    press(&win, "f");
    assert!(st.get_keys_note().contains("break typing"));
    press(&win, &key(Key::Escape));
    // help screen shows the current bindings
    press(&win, &key(Key::F1));
    let help_keys: Vec<String> =
        st.get_help_left().iter().chain(st.get_help_right().iter()).map(|r| r.keys.to_string()).collect();
    assert!(help_keys.iter().any(|k| k == "Ctrl+Shift+M"), "{help_keys:?}");
    shot(&win, "20-help-custom");
    press(&win, &key(Key::Escape));
    // the startup override from the settings file is live: Ctrl+9 resets the size, Ctrl+0 does not
    ctrl(&win, "=");
    ctrl(&win, "0");
    assert_eq!(ui.global::<Palette>().get_fs(), 14.0, "Ctrl+0 was moved away by the settings file");
    ctrl(&win, "9");
    assert_eq!(ui.global::<Palette>().get_fs(), 13.0, "Ctrl+9 resets, as the settings file says");
    // reset everything (the startup override for zoom_reset goes too)
    st.invoke_keys_reset_all();
    assert!(Config::load(&cfg_path).keys.is_empty());
    assert_eq!(st.get_hints().row_data(0).unwrap().key, "Ctrl+N");
    ctrl(&win, "k");
    assert_eq!(st.get_tab(), 0, "Ctrl+K toggles the tab");
    // back on dc01 for the template test below
    press(&win, &key(Key::Escape));
    press(&win, &key(Key::Home));
    assert_eq!(st.get_draft_label(), "dc01");

    // ctrl+l flips the theme and persists it
    assert!(ui.global::<Palette>().get_dark());
    ctrl(&win, "l");
    assert!(!ui.global::<Palette>().get_dark());
    assert_eq!(Config::load(&cfg_path).theme, "light");
    shot(&win, "1a-light");
    ctrl(&win, "l");
    assert_eq!(Config::load(&cfg_path).theme, "dark");

    // template fill-in pulls host/user from the selected entry
    ctrl(&win, "t");
    let vars: Vec<(String, String)> =
        st.get_tpl_vars().iter().map(|v| (v.name.to_string(), v.value.to_string())).collect();
    assert_eq!(
        vars,
        [("user".into(), "administrator".into()), ("host".into(), "10.10.10.5".into()), ("port".into(), "".into())]
    );
    assert_eq!(st.get_tpl_preview(), "ssh administrator@10.10.10.5 -p {port}");
    shot(&win, "1b-template-fill");
    press(&win, &key(Key::Escape));
    assert!(!st.get_show_templates());

    // down arrow selects the next entry
    press(&win, &key(slint::platform::Key::DownArrow));
    assert_eq!(st.get_selected(), 1);
    assert_eq!(st.get_draft_label(), "web01");

    // ctrl+n adds an entry and focuses its label; typing fills it in
    ctrl(&win, "n");
    assert_eq!(st.get_selected(), 2);
    type_str(&win, "sw-core");
    assert_eq!(st.get_draft_label(), "sw-core");
    assert!(st.get_dirty());
    shot(&win, "2-new-entry");

    // ctrl+s checkpoints to disk
    ctrl(&win, "s");
    assert!(!st.get_status_error());
    assert!(!st.get_dirty());

    // ctrl+t opens templates, esc closes
    ctrl(&win, "t");
    assert!(st.get_show_templates());
    shot(&win, "3-templates");
    press(&win, &key(slint::platform::Key::Escape));
    assert!(!st.get_show_templates());

    // ---- quick-list popup ----
    press(&win, &key(Key::Escape));
    let quick = windows.borrow()[1].clone();
    let ps = app.quick_state();
    app.show_popup();
    assert_eq!(ps.get_rows().row_count(), 3, "empty query lists everything");
    assert_eq!(ps.get_selected(), 0);
    shot(&quick, "5-popup");
    press(&quick, &key(Key::DownArrow));
    assert_eq!(ps.get_selected(), 1, "arrows work while the search box has focus");
    press(&quick, &key(Key::UpArrow));
    assert_eq!(ps.get_selected(), 0);
    type_str(&quick, "10.10");
    assert_eq!(ps.get_query(), "10.10");
    assert_eq!(ps.get_rows().row_count(), 1, "search matches field values");
    assert_eq!(ps.get_rows().row_data(0).unwrap().label, "dc01");
    assert_eq!(ps.get_fields().row_count(), 3);
    shot(&quick, "6-popup-search");
    // inline edit writes through to the vault and the main window
    ps.invoke_field_edited(0, "10.10.10.99".into());
    assert_eq!(st.get_draft_fields().row_data(0).unwrap().value, "10.10.10.99");
    press(&quick, &key(Key::Escape));
    assert!(!app.quick_visible());
    // no matches
    app.show_popup();
    type_str(&quick, "zzz");
    assert_eq!(ps.get_rows().row_count(), 0);
    assert_eq!(ps.get_selected(), -1);
    press(&quick, &key(Key::Escape));

    // ---- another instance saves the same vault: refuse, then overwrite on 2nd ^s ----
    {
        let path = vaults_dir.join("lab.db");
        let mut theirs = paddy_core::Vault::open(&path).unwrap();
        theirs.add_entry(&Entry::new("from-other-instance")).unwrap();
        theirs.save().unwrap();
    }
    ctrl(&win, "n");
    type_str(&win, "mine");
    ctrl(&win, "s");
    assert!(st.get_status_error(), "first save is refused");
    assert!(st.get_status().contains("again to overwrite"));
    assert!(st.get_dirty());
    ctrl(&win, "s");
    assert!(!st.get_status_error());
    assert!(!st.get_dirty());
    press(&win, &key(Key::Escape));
    // remove the scratch entry again so the counts below stay as they were
    ctrl(&win, "d");
    press(&win, "y");

    // ---- settings tab ----
    ctrl(&win, ",");
    assert_eq!(st.get_tab(), 1);
    assert_eq!(st.get_settings_hotkey(), "ctrl+alt+p");
    assert!(st.get_config_path().ends_with("config"));
    assert!(st.get_vaults_path().ends_with("vaults"));
    assert!(st.get_diag_text().contains("memory"), "{}", st.get_diag_text());
    shot(&win, "10-settings");
    st.set_settings_hotkey("banana".into());
    st.invoke_apply_hotkey();
    assert!(st.get_settings_note().contains("not a valid hotkey"));
    st.set_settings_hotkey("ctrl+shift+space".into());
    st.invoke_apply_hotkey();
    assert!(st.get_settings_note().contains("Restart"));
    assert_eq!(Config::load(&cfg_path).hotkey, "ctrl+shift+space");
    // window bar mode is persisted and switches the built-in bar
    assert!(st.get_builtin_bar());
    st.invoke_set_window_bar("native".into());
    assert!(!st.get_builtin_bar());
    assert_eq!(Config::load(&cfg_path).window_bar, WindowBar::Native);
    st.invoke_set_window_bar("builtin".into());
    assert!(st.get_builtin_bar());
    assert_eq!(st.get_window_bar_mode(), "builtin");
    // floating launcher button
    let mini = windows.borrow()[2].clone();
    assert!(!app.mini_visible());
    st.invoke_toggle_mini();
    assert!(app.mini_visible() && st.get_mini_on());
    assert!(Config::load(&cfg_path).mini_button);
    shot(&mini, "11-mini");
    st.invoke_toggle_mini();
    assert!(!app.mini_visible());
    // theme buttons set (not flip) the theme
    st.invoke_set_theme(false);
    st.invoke_set_theme(false);
    assert_eq!(Config::load(&cfg_path).theme, "light");
    shot(&win, "10b-settings-light");
    st.invoke_set_theme(true);
    assert_eq!(Config::load(&cfg_path).theme, "dark");
    // built-in bar: resize grip callback changes the window size, clamped to the minimum
    let before = win.size();
    ui.invoke_resize_window(40.0, 30.0);
    assert_eq!(win.size().width, before.width + 40);
    ui.invoke_resize_window(-5000.0, -5000.0);
    assert_eq!((win.size().width, win.size().height), (780, 460));
    ui.invoke_resize_window(220.0, 180.0);
    // escape leaves the settings tab
    press(&win, &key(Key::Escape));
    assert_eq!(st.get_tab(), 0);
    ctrl(&win, ",");
    assert_eq!(st.get_tab(), 1);
    ctrl(&win, ",");
    assert_eq!(st.get_tab(), 0, "Ctrl+, toggles");
    ctrl(&win, ",");
    ctrl(&win, "n");
    assert_eq!(st.get_tab(), 0, "Ctrl+N from settings jumps back to entries");
    ctrl(&win, "d");
    press(&win, "y");

    // ---- themes: picker with live preview, toggle, and the 12 built-ins ----
    let pal = ui.global::<Palette>();
    let dark_bg = pal.get_bg();
    st.invoke_open_themes();
    assert!(st.get_show_themes());
    assert!(st.get_tp_rows().row_count() >= 12);
    let nord = st.get_tp_rows().iter().position(|r| r.primary == "Nord").unwrap();
    st.invoke_tp_select(nord as i32);
    assert_eq!(pal.get_bg(), slint::Color::from_rgb_u8(0x2e, 0x34, 0x40), "arrowing previews the theme live");
    assert_eq!(app.quick_palette_bg(), pal.get_bg(), "popup follows the preview");
    shot(&win, "23-theme-nord");
    press(&win, &key(Key::Escape));
    assert_eq!(pal.get_bg(), dark_bg, "Esc restores the saved theme");
    assert_eq!(Config::load(&cfg_path).theme, "dark");
    st.invoke_open_themes();
    st.invoke_tp_select(nord as i32);
    press(&win, &key(Key::Return));
    assert_eq!(Config::load(&cfg_path).theme, "nord");
    assert_eq!(st.get_theme_name(), "Nord");
    ctrl(&win, "l");
    assert_eq!(Config::load(&cfg_path).theme, "light", "Ctrl+L from a dark theme goes to the light default");
    assert!(!pal.get_dark());
    ctrl(&win, "l");
    assert_eq!(Config::load(&cfg_path).theme, "dark");
    // every built-in theme paints without trouble
    st.invoke_open_themes();
    assert!(st.get_show_themes());
    let count = st.get_tp_rows().row_count();
    for i in 0..count {
        st.invoke_tp_select(i as i32);
    }
    press(&win, &key(Key::Escape));
    assert!(!st.get_show_themes(), "Esc closes the picker after previewing everything");
    assert_eq!(pal.get_bg(), dark_bg, "and the saved theme is back");
    assert_eq!(pal.get_accent(), slint::Color::from_rgb_u8(0x8c, 0xaa, 0x89));

    // reopening the picker with a non-first theme saved: highlight and painted theme must agree
    st.invoke_open_themes();
    let sol = st.get_tp_rows().iter().position(|r| r.primary == "Solarized light").unwrap();
    st.invoke_tp_select(sol as i32);
    press(&win, &key(Key::Return));
    assert_eq!(Config::load(&cfg_path).theme, "solarized-light");
    let saved_bg = pal.get_bg();
    st.invoke_open_themes();
    assert_eq!(st.get_tp_selected(), sol as i32, "opens on the current theme");
    assert_eq!(pal.get_bg(), saved_bg, "and shows that theme, not the first row's");
    press(&win, &key(Key::Escape));
    assert_eq!(pal.get_bg(), saved_bg);
    // arrowing to the bottom of the list keeps the highlighted row on screen
    st.invoke_open_themes();
    st.invoke_tp_select(0);
    for _ in 0..20 {
        press(&win, &key(Key::DownArrow));
    }
    let last = st.get_tp_rows().row_count() as i32 - 1;
    assert_eq!(st.get_tp_selected(), last);
    assert_eq!(st.get_theme_name(), "Solarized light", "previewing doesn't change the saved theme name");
    shot(&win, "26-picker-scrolled");
    press(&win, &key(Key::Home));
    st.invoke_tp_select(0);
    press(&win, &key(Key::Return));

    // ---- packs tab ----
    ctrl(&win, "p");
    assert_eq!(st.get_tab(), 3);
    assert_eq!(st.get_pk_fonts().row_count(), 6);
    assert!(st.get_pk_fonts().iter().all(|f| !f.installed), "nothing downloaded yet");
    assert_eq!(st.get_pk_packs().row_count(), 4);
    shot(&win, "24-packs");
    // built-in template pack: adds, then is idempotent
    let templates_before = {
        st.invoke_open_templates();
        let n = st.get_templates().row_count();
        st.invoke_close_templates();
        n
    };
    st.invoke_pack_add("recon-enum".into());
    assert!(st.get_net_note().contains("added"), "{}", st.get_net_note());
    st.invoke_pack_add("recon-enum".into());
    assert!(st.get_net_note().contains("already"), "{}", st.get_net_note());
    st.invoke_open_templates();
    assert!(st.get_templates().row_count() >= templates_before + 15);
    st.invoke_close_templates();
    // a font that is not downloaded can't be "used"; a tampered file is not treated as installed
    st.invoke_font_use("hack".into());
    assert!(st.get_net_note().contains("not downloaded"), "{}", st.get_net_note());
    std::fs::create_dir_all(&fonts_dir).unwrap();
    std::fs::write(fonts_dir.join("Hack-Regular.ttf"), b"not a real font").unwrap();
    st.invoke_open_packs();
    assert!(
        st.get_pk_fonts().iter().find(|f| f.id == "hack").is_some_and(|f| !f.installed),
        "fake file isn't accepted"
    );
    app.register_fonts();
    assert!(
        st.get_status_error() && st.get_status().contains("modified"),
        "tampered font is refused: {}",
        st.get_status()
    );
    // importing a theme from a URL: shown for confirmation first, nothing saved until Enter
    let good = "id = ocean\nname = Ocean night\ndark = true\nbg = #0b1622\npanel = #0f1e2e\nfg = #d6e2f0\ndim = #7d92a8\nline = #23384d\naccent = #4fc3f7\nsel = #15293b\nwarn = #ff8a65\n";
    app.debug_deliver("theme", "https://example.org/themes/ocean.theme", good.as_bytes().to_vec());
    assert!(st.get_show_import());
    assert!(
        st.get_import_body().contains("Ocean night")
            && st.get_import_body().contains("example.org")
            && st.get_import_body().contains("sha256")
    );
    shot(&win, "25-import-confirm");
    press(&win, &key(Key::Escape));
    assert!(!st.get_show_import());
    assert!(!themes_dir.join("ocean.theme").exists(), "cancel saves nothing");
    app.debug_deliver("theme", "https://example.org/themes/ocean.theme", good.as_bytes().to_vec());
    press(&win, &key(Key::Return));
    assert!(themes_dir.join("ocean.theme").exists());
    assert_eq!(Config::load(&cfg_path).theme, "ocean");
    // importing the same id again warns that it replaces the existing one
    app.debug_deliver("theme", "https://example.org/themes/ocean.theme", good.as_bytes().to_vec());
    assert!(st.get_import_body().contains("replaces your existing theme"), "{}", st.get_import_body());
    press(&win, &key(Key::Escape));
    assert_eq!(st.get_theme_name(), "Ocean night");
    // hostile or broken downloads are refused with a reason, and never reach the dialog
    for (bytes, needle) in [
        (good.replace("id = ocean", "id = dark").into_bytes(), "built-in"),
        (good.replace("fg = #d6e2f0", "fg = #0c1723").into_bytes(), "hard to read"),
        (good.replace("bg = #0b1622", "bg = navy").into_bytes(), "#rrggbb"),
        (vec![0xff, 0xfe, 0x00, 0x80], "UTF-8"),
        (b"just some html <script>alert(1)</script>".to_vec(), "missing"),
    ] {
        app.debug_deliver("theme", "https://evil.example/x", bytes);
        if st.get_show_import() {
            // built-in id parses fine; it is refused when saving
            press(&win, &key(Key::Return));
        }
        assert!(st.get_net_note().contains(needle), "wanted {needle:?} in {:?}", st.get_net_note());
    }
    assert!(!themes_dir.join("dark.theme").exists(), "a downloaded theme can't replace a built-in one");
    // template pack from a URL
    let pack = "id = my-pack\nname = My pack\nauthor = me\nlicense = MIT\n[templates]\nping host = ping -c1 {host}\ntrace = mtr -rw {host}\n";
    app.debug_deliver("templates", "https://example.org/p.pack", pack.as_bytes().to_vec());
    assert!(
        st.get_show_import()
            && st.get_import_body().contains("My pack")
            && st.get_import_body().contains("ping -c1 {host}")
    );
    press(&win, &key(Key::Return));
    assert!(st.get_net_note().contains("added 2"), "{}", st.get_net_note());
    app.debug_deliver("templates", "https://example.org/p.pack", pack.as_bytes().to_vec());
    press(&win, &key(Key::Return));
    assert!(st.get_net_note().contains("already"), "{}", st.get_net_note());
    // a pack hiding a right-to-left override in a command is refused outright
    let evil = pack.replace("ping -c1", "ping \u{202e}-c1");
    app.debug_deliver("templates", "https://evil.example/p.pack", evil.into_bytes());
    assert!(!st.get_show_import(), "deceptive text never reaches the dialog");
    assert!(st.get_net_note().contains("invisible"), "{}", st.get_net_note());
    // URL entry checks happen before any request
    st.set_import_url("http://example.com/x.theme".into());
    st.invoke_import_theme_url();
    assert!(st.get_net_note().contains("https"), "{}", st.get_net_note());
    st.set_import_url("https://127.0.0.1/x.theme".into());
    st.invoke_import_theme_url();
    assert!(!st.get_net_busy() || pump_until(|| !st.get_net_busy()));
    assert!(pump_until(|| !st.get_net_busy()), "the job ends");
    assert!(st.get_net_note().contains("non-public"), "private addresses are refused: {}", st.get_net_note());
    st.invoke_import_theme_url();
    st.set_import_url("".into());
    press(&win, &key(Key::Escape));
    // put a normal theme back for the screenshots below
    st.invoke_open_themes();
    st.invoke_tp_select(0);
    press(&win, &key(Key::Return));

    // ---- privacy: clipboard auto-clear setting ----
    assert_eq!(st.get_clip_secs(), 30, "on by default");
    st.invoke_set_clip_secs(60);
    assert_eq!(Config::load(&cfg_path).clip_clear_secs, 60);
    st.invoke_set_clip_secs(0);
    assert_eq!(Config::load(&cfg_path).clip_clear_secs, 0);
    st.invoke_set_clip_secs(30);

    // ---- look: zoom, density, fonts, animation ----
    let pal = ui.global::<Palette>();
    assert_eq!(pal.get_fs(), 13.0);
    ctrl(&win, "=");
    assert_eq!(pal.get_fs(), 14.0);
    assert_eq!(st.get_font_size_px(), 14);
    assert_eq!(Config::load(&cfg_path).font_size, 14);
    ctrl(&win, "-");
    ctrl(&win, "-");
    assert_eq!(pal.get_fs(), 12.0);
    for _ in 0..30 {
        ctrl(&win, "-");
    }
    assert_eq!(pal.get_fs(), 10.0, "clamped at the minimum");
    for _ in 0..30 {
        ctrl(&win, "=");
    }
    assert_eq!(pal.get_fs(), 22.0, "clamped at the maximum");
    shot(&win, "14-big-text");
    ctrl(&win, "0");
    assert_eq!(pal.get_fs(), 13.0);
    // the popup and launcher follow
    assert_eq!(app.quick_palette_fs(), 13.0);
    st.invoke_set_density("compact".into());
    assert!((pal.get_row() - 13.0 * Density::Compact.row_scale()).abs() < 0.01);
    st.invoke_set_density("roomy".into());
    assert_eq!(Config::load(&cfg_path).density, Density::Roomy);
    st.invoke_set_density("nonsense".into());
    assert_eq!(Config::load(&cfg_path).density, Density::Roomy, "unknown density ignored");
    // density visibly changes the screen on the settings tab too (gaps and padding, not just rows)
    ctrl(&win, ",");
    let mut frames = Vec::new();
    let mut gaps = Vec::new();
    for d in ["compact", "normal", "roomy"] {
        st.invoke_set_density(d.into());
        gaps.push(pal.get_gap());
        frames.push(frame_hash(&win));
    }
    assert!(gaps[0] < gaps[1] && gaps[1] < gaps[2], "gap grows with density: {gaps:?}");
    assert!((gaps[1] - 8.0).abs() < 0.01);
    assert!(
        frames[0] != frames[1] && frames[1] != frames[2] && frames[0] != frames[2],
        "each density draws differently"
    );
    shot(&win, "21-roomy-settings");
    st.invoke_set_density("compact".into());
    shot(&win, "22-compact-settings");
    // font size changes text only: the window keeps its size
    let win_size = win.size();
    ctrl(&win, "=");
    ctrl(&win, "=");
    assert_eq!(win.size(), win_size, "font size must not resize the window");
    ctrl(&win, "0");
    ctrl(&win, ",");
    st.invoke_set_density("normal".into());

    // font picker: arrows preview live, Esc restores, Enter keeps
    let saved_font = Config::load(&cfg_path).font;
    st.invoke_open_fonts();
    assert!(st.get_show_fonts());
    assert!(st.get_fp_rows().row_count() > 0);
    shot(&win, "15-fonts");
    let last = st.get_fp_rows().row_count() - 1;
    st.invoke_fp_select(last as i32);
    let previewed = st.get_fp_rows().row_data(last).unwrap().primary;
    assert_eq!(pal.get_font(), previewed, "preview applies immediately");
    assert_eq!(Config::load(&cfg_path).font, saved_font, "preview is not saved");
    press(&win, &key(Key::Escape));
    assert!(!st.get_show_fonts());
    assert_eq!(pal.get_font(), saved_font, "Esc restores the saved font");
    st.invoke_open_fonts();
    st.invoke_fp_select(last as i32);
    press(&win, &key(Key::Return));
    assert!(!st.get_show_fonts());
    assert_eq!(Config::load(&cfg_path).font, previewed.as_str());
    assert_eq!(st.get_font_name(), previewed);
    // filtering narrows the list
    st.invoke_open_fonts();
    st.set_fp_query("zzzz-no-such-font".into());
    st.invoke_fp_edited();
    assert_eq!(st.get_fp_rows().row_count(), 0);
    press(&win, &key(Key::Escape));
    // put the default back for the screenshots below
    st.invoke_open_fonts();
    st.set_fp_query(saved_font.clone().into());
    st.invoke_fp_edited();
    press(&win, &key(Key::Return));

    // logo animation: plays on save, ends by itself, and leaves no timer running
    assert!(!st.get_logo().is_empty() && !st.get_logo_mini().is_empty());
    assert!(pump_until(|| !app.logo_animating()), "startup animation finishes");
    let idle_logo = st.get_logo();
    assert_eq!(idle_logo, paddy_ui::logo::big_path(0));
    ctrl(&win, "s");
    assert!(app.logo_animating(), "save starts the animation");
    assert_ne!(st.get_logo_mini(), paddy_ui::logo::mini_path(0));
    assert!(pump_until(|| !app.logo_animating()), "and it stops by itself");
    assert_eq!(st.get_logo(), idle_logo, "back to the idle logo");
    assert_eq!(st.get_logo_mini(), paddy_ui::logo::mini_path(0));
    // animations off: nothing starts
    st.invoke_toggle_anim();
    assert!(!st.get_anim_on());
    ctrl(&win, "s");
    assert!(!app.logo_animating(), "no timer when animations are off");
    st.invoke_toggle_anim();
    assert!(st.get_anim_on());
    assert!(pump_until(|| !app.logo_animating()));

    // ---- shortcut help ----
    press(&win, &key(Key::F1));
    assert!(st.get_show_help());
    shot(&win, "12-help");
    press(&win, &key(Key::Escape));
    assert!(!st.get_show_help());

    // ---- per-vault search ----
    assert_eq!(st.get_entries().row_count(), 3);
    ctrl(&win, "f");
    type_str(&win, "web");
    assert_eq!(st.get_search(), "web");
    assert_eq!(st.get_entries().row_count(), 1);
    assert_eq!(st.get_draft_label(), "web01");
    shot(&win, "7-search");
    press(&win, &key(Key::Escape));
    assert_eq!(st.get_search(), "");
    assert_eq!(st.get_entries().row_count(), 3);

    // ---- global search jumps into another vault ----
    ctrl(&win, "g");
    assert!(st.get_show_global());
    type_str(&win, "192.168");
    assert_eq!(st.get_gs_rows().row_count(), 1);
    assert!(st.get_gs_preview().contains("host = 192.168.1.1"));
    shot(&win, "8-global");
    press(&win, &key(Key::Return));
    assert!(!st.get_show_global());
    assert_eq!(st.get_vault_name(), "other");
    assert_eq!(st.get_draft_label(), "fw01");
    assert_eq!(app.quick_state().get_vault_name(), "other", "popup follows the vault switch");
    assert_eq!(app.quick_state().get_rows().row_count(), 1);
    assert_eq!(Config::load(&cfg_path).last_vault.unwrap().file_name().unwrap(), "other.db");

    // ---- vault switcher: switch back, then create ----
    ctrl(&win, "o");
    assert!(st.get_show_vaults());
    assert_eq!(st.get_vs_rows().row_count(), 2);
    shot(&win, "9-vaults");
    type_str(&win, "lab");
    assert_eq!(st.get_vs_rows().row_count(), 1, "filtered, and no create row for an exact name");
    press(&win, &key(Key::Return));
    assert_eq!(st.get_vault_name(), "lab");
    assert_eq!(st.get_entries().row_count(), 3, "unsaved-free: switching kept our entries");

    ctrl(&win, "o");
    type_str(&win, "new lab");
    assert_eq!(st.get_vs_rows().row_count(), 1);
    assert!(st.get_vs_rows().row_data(0).unwrap().primary.contains("create"));
    press(&win, &key(Key::Return));
    assert_eq!(st.get_vault_name(), "new lab");
    assert_eq!(st.get_entries().row_count(), 0);
    shot(&win, "13-empty");
    assert!(vaults_dir.join("new-lab.db").exists());
    // esc closes an overlay without side effects
    ctrl(&win, "o");
    press(&win, &key(Key::Escape));
    assert!(!st.get_show_vaults());
    // back to lab for the delete flow
    ctrl(&win, "o");
    type_str(&win, "lab");
    let want = st.get_vs_rows().iter().position(|r| r.primary == "lab").unwrap();
    for _ in 0..want {
        press(&win, &key(Key::DownArrow));
    }
    press(&win, &key(Key::Return));
    assert_eq!(st.get_vault_name(), "lab");

    // ctrl+d asks, n keeps, ctrl+d y deletes
    press(&win, &key(slint::platform::Key::Escape));
    ctrl(&win, "d");
    assert!(st.get_confirm_delete());
    shot(&win, "4-confirm");
    press(&win, "n");
    assert!(!st.get_confirm_delete());
    assert_eq!(app_count(&st), 3);
    ctrl(&win, "d");
    press(&win, "y");
    assert_eq!(st.get_entries().row_count(), 2);
    let _ = app;
}

fn app_count(st: &AppState) -> usize {
    st.get_entries().row_count()
}
