//! Real download + registration of a font. Needs internet, so it is ignored by default:
//!   cargo test -p paddy-ui --test fonts_live -- --ignored
use std::cell::RefCell;
use std::rc::Rc;

use paddy_core::{find_font, font_installed, store_font};
use paddy_net::{fetch_verified, Policy};
use paddy_ui::fonts;
use slint::fontique_011::shared_collection;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};

thread_local! {
    static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
}

struct Headless;
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let w = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        w.set_size(slint::PhysicalSize::new(600, 200));
        WINDOW.with(|c| *c.borrow_mut() = Some(w.clone()));
        Ok(w)
    }
}

fn render(font: &str, name: &str) -> Vec<u8> {
    use slint::platform::software_renderer::PremultipliedRgbaColor;
    use slint::ComponentHandle;
    let ui = paddy_ui::MainWindow::new().unwrap();
    ui.global::<paddy_ui::Palette>().set_font(font.into());
    ui.show().unwrap();
    let win = WINDOW.with(|c| c.borrow().clone().unwrap());
    let mut buf = vec![PremultipliedRgbaColor::default(); 600 * 200];
    win.request_redraw();
    win.draw_if_needed(|r| {
        r.render(&mut buf, 600);
    });
    let rgb: Vec<u8> = buf.iter().flat_map(|p| [p.red, p.green, p.blue]).collect();
    if let Some(dir) = std::env::var_os("PADDY_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
        let mut enc = png::Encoder::new(file, 600, 200);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgb).unwrap();
    }
    rgb
}

#[test]
#[ignore = "needs internet"]
fn download_verify_store_register_and_the_family_appears() {
    slint::platform::set_platform(Box::new(Headless)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let pack = find_font("jetbrains-mono").unwrap();
    assert!(!font_installed(dir.path(), pack));

    for f in pack.files {
        let bytes = fetch_verified(f.url, f.sha256, f.size + 1, &Policy::default()).expect("download + checksum");
        store_font(dir.path(), f, &bytes).expect("store");
    }
    assert!(font_installed(dir.path(), pack));

    let mut done = std::collections::HashSet::new();
    let warnings = fonts::register_downloaded(dir.path(), &mut done);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(done.contains("jetbrains-mono"));

    // the shared collection Slint renders from now knows the family
    let mut collection = shared_collection();
    assert!(collection.family_id("JetBrains Mono").is_some(), "family should be registered");
    println!("families now include: {:?}", fonts::downloaded_families(dir.path()));

    // and it really draws differently from the default font
    let default = render("DejaVu Sans Mono", "font-default");
    let jetbrains = render("JetBrains Mono", "font-jetbrains");
    assert_ne!(default, jetbrains, "the downloaded font must change the rendering");

    // tampering after install: the file is no longer trusted or loaded again
    let path = dir.path().join(pack.files[0].file);
    let mut data = std::fs::read(&path).unwrap();
    data[1000] ^= 0xff;
    std::fs::write(&path, data).unwrap();
    assert!(!font_installed(dir.path(), pack));
    let mut fresh = std::collections::HashSet::new();
    let warnings = fonts::register_downloaded(dir.path(), &mut fresh);
    assert!(warnings.iter().any(|w| w.contains("modified")), "{warnings:?}");
    assert!(fresh.is_empty(), "a tampered pack is never registered");
}
