//! Regenerate the README screenshots and GIFs from the real UI (headless, no display needed):
//!   cargo run --release -p paddy-ui --example record_media
//! Writes to docs/media/. The demo vault is made-up data (documentation IP ranges,
//! placeholder passwords).

use std::cell::RefCell;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use paddy_core::{create_vault, Config, Entry, Field, Paths, WindowBar};
use paddy_ui::{app::App, AppState, MainWindow};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Key, Platform, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, Model, PhysicalSize, SharedString};

const W: u32 = 960;
const H: u32 = 600;

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

struct Frame {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
}

fn pump(d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end {
        slint::platform::update_timers_and_animations();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn grab(win: &MinimalSoftwareWindow) -> Frame {
    pump(Duration::from_millis(30));
    let s = win.size();
    let (w, h) = (s.width, s.height);
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    win.request_redraw();
    win.draw_if_needed(|r| {
        r.render(&mut buf, w as usize);
    });
    Frame { rgba: buf.iter().flat_map(|p| [p.red, p.green, p.blue, 255]).collect(), w, h }
}

fn save_png(f: &Frame, path: &Path) {
    let mut e = png::Encoder::new(File::create(path).unwrap(), f.w, f.h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.set_compression(png::Compression::Best);
    e.write_header().unwrap().write_image_data(&f.rgba).unwrap();
    println!("{} ({} KB)", path.display(), std::fs::metadata(path).unwrap().len() / 1024);
}

/// Frames with per-frame delay in 1/100 s. Identical consecutive frames are merged.
fn save_gif(frames: &[(Frame, u16)], path: &Path) {
    let (w, h) = (frames[0].0.w as u16, frames[0].0.h as u16);
    let mut enc = gif::Encoder::new(File::create(path).unwrap(), w, h, &[]).unwrap();
    enc.set_repeat(gif::Repeat::Infinite).unwrap();
    let mut i = 0;
    while i < frames.len() {
        let mut delay = frames[i].1;
        let mut j = i + 1;
        while j < frames.len() && frames[j].0.rgba == frames[i].0.rgba {
            delay += frames[j].1;
            j += 1;
        }
        let mut rgba = frames[i].0.rgba.clone();
        let mut fr = gif::Frame::from_rgba_speed(w, h, &mut rgba, 10);
        fr.delay = delay;
        enc.write_frame(&fr).unwrap();
        i = j;
    }
    drop(enc);
    println!("{} ({} KB)", path.display(), std::fs::metadata(path).unwrap().len() / 1024);
}

fn key(k: Key) -> String {
    SharedString::from(k).to_string()
}

fn press(win: &MinimalSoftwareWindow, text: &str) {
    let t: SharedString = text.into();
    win.dispatch_event(WindowEvent::KeyPressed { text: t.clone() });
    win.dispatch_event(WindowEvent::KeyReleased { text: t });
}

fn ctrl(win: &MinimalSoftwareWindow, k: &str) {
    win.dispatch_event(WindowEvent::KeyPressed { text: Key::Control.into() });
    press(win, k);
    win.dispatch_event(WindowEvent::KeyReleased { text: Key::Control.into() });
}

fn entry(label: &str, tags: &[&str], fields: &[(&str, &str, bool)], notes: &str) -> Entry {
    let mut e = Entry::new(label);
    e.tags = tags.iter().map(|t| t.to_string()).collect();
    e.fields = fields.iter().map(|(k, v, s)| if *s { Field::secret(*k, *v) } else { Field::new(*k, *v) }).collect();
    e.notes = notes.into();
    e
}

fn main() {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/media");
    std::fs::create_dir_all(&out).unwrap();
    let platform = Headless::default();
    let windows = platform.0.clone();
    slint::platform::set_platform(Box::new(platform)).unwrap();

    // ---- demo data: documentation address ranges, placeholder secrets
    let tmp = tempfile::tempdir().unwrap();
    let vaults = tmp.path().join("vaults");
    create_vault(&vaults, "branch-office").unwrap().save().unwrap();
    let mut v = create_vault(&vaults, "lab").unwrap();
    let core_notes = "# Core switch\nUplinks on **Te1/1/1-2**, LACP to the firewall.\n\n- [x] backup running-config\n- [x] NTP to 192.0.2.10\n- [ ] upgrade to 17.9\n\n> change window: Sunday 02:00\n\n```\nshow interfaces status | include connected\n```";
    for e in [
        entry(
            "core-sw01",
            &["cisco", "core"],
            &[
                ("host", "192.0.2.1", false),
                ("user", "netadmin", false),
                ("password", "example-pass", true),
                ("port", "22", false),
                ("interface", "Te1/1/1", false),
            ],
            core_notes,
        ),
        entry(
            "fw-edge",
            &["firewall"],
            &[("host", "192.0.2.254", false), ("user", "admin", false), ("password", "example-pass", true)],
            "HA pair, active unit is **fw-edge-a**.",
        ),
        entry(
            "web01",
            &["linux", "web"],
            &[("host", "198.51.100.21", false), ("user", "deploy", false), ("url", "https://web01.example.net", false)],
            "nginx + certbot, logs in `/var/log/nginx`",
        ),
        entry(
            "jumpbox",
            &["linux", "ssh"],
            &[("host", "198.51.100.40", false), ("user", "ops", false), ("port", "2222", false)],
            "ProxyJump for the lab network",
        ),
        entry(
            "vpn",
            &["remote"],
            &[("host", "203.0.113.7", false), ("interface", "tun0", false)],
            "`sudo openvpn lab.ovpn`",
        ),
    ] {
        v.add_entry(&e).unwrap();
    }
    v.import_templates(&paddy_core::builtin_packs()[0].templates, "net").unwrap();
    v.save().unwrap();

    let paths = Paths {
        config_file: tmp.path().join("config"),
        vaults_dir: vaults,
        themes_dir: tmp.path().join("themes"),
        fonts_dir: tmp.path().join("fonts"),
    };
    let cfg = Config { window_bar: WindowBar::Native, ..Config::default() };
    let ui = MainWindow::new().unwrap();
    let app = App::new(&ui, v, cfg, paths);
    ui.show().unwrap();
    let win = windows.borrow()[0].clone();
    let quick = windows.borrow()[1].clone();
    let st = ui.global::<AppState>();
    pump(Duration::from_millis(1200)); // let the startup animation finish

    // ---- hero: an entry with its markdown notes previewed
    ctrl(&win, "e");
    save_png(&grab(&win), &out.join("main.png"));

    // ---- notes: edit <-> preview
    let mut frames = vec![(grab(&win), 180)];
    ctrl(&win, "e");
    frames.push((grab(&win), 150));
    ctrl(&win, "e");
    frames.push((grab(&win), 180));
    save_gif(&frames, &out.join("notes.gif"));
    ctrl(&win, "e");

    // ---- templates: filled in from the selected entry
    let mut frames = vec![(grab(&win), 80)];
    ctrl(&win, "t");
    frames.push((grab(&win), 150));
    let names: Vec<String> = st.get_templates().iter().map(|t| t.name.to_string()).collect();
    for want in ["SSH connect", "Cisco: interface detail", "Linux: tcpdump host", "SSH via jump host"] {
        if let Some(i) = names.iter().position(|n| n == want) {
            st.invoke_tpl_select(i as i32);
            frames.push((grab(&win), 160));
        }
    }
    save_png(&frames[frames.len() - 3].0, &out.join("templates.png"));
    save_gif(&frames, &out.join("templates.gif"));
    press(&win, &key(Key::Escape));

    // ---- search this vault
    ctrl(&win, "f");
    let mut frames = vec![(grab(&win), 60)];
    for c in "linux".chars() {
        press(&win, &c.to_string());
        frames.push((grab(&win), 22));
    }
    frames.last_mut().unwrap().1 = 160;
    press(&win, &key(Key::Escape));
    frames.push((grab(&win), 80));
    save_gif(&frames, &out.join("search.gif"));

    // ---- quick list (its own window)
    app.show_popup();
    let mut frames = vec![(grab(&quick), 80)];
    for c in "198.51".chars() {
        press(&quick, &c.to_string());
        frames.push((grab(&quick), 20));
    }
    frames.last_mut().unwrap().1 = 120;
    press(&quick, &key(Key::DownArrow));
    frames.push((grab(&quick), 160));
    save_png(&frames.last().unwrap().0, &out.join("quick.png"));
    save_gif(&frames, &out.join("quick.gif"));
    press(&quick, &key(Key::Escape));

    // ---- themes: live preview while arrowing through the picker
    app.open_themes();
    let count = st.get_tp_rows().row_count();
    let mut frames = Vec::new();
    for i in 0..count {
        st.invoke_tp_select(i as i32);
        frames.push((grab(&win), 90));
    }
    save_gif(&frames, &out.join("themes.gif"));
    press(&win, &key(Key::Escape));

    // ---- light cream and the other tabs
    ctrl(&win, "l");
    ctrl(&win, "e");
    save_png(&grab(&win), &out.join("light.png"));
    ctrl(&win, "e");
    ctrl(&win, "l");
    ctrl(&win, "p");
    save_png(&grab(&win), &out.join("packs.png"));
    ctrl(&win, "k");
    save_png(&grab(&win), &out.join("keys.png"));
    ctrl(&win, "k");
    ctrl(&win, ",");
    save_png(&grab(&win), &out.join("settings.png"));

    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/icons/paddy-128.png"),
        out.join("icon.png"),
    )
    .unwrap();
    drop(app);
}
