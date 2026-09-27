//! Regenerate the icon files in assets/icons from the logo geometry:
//!   cargo run -p paddy-ui --example make_icons
use std::fs::File;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/icons");
    std::fs::create_dir_all(&dir).unwrap();
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512] {
        let path = dir.join(format!("paddy-{size}.png"));
        let mut enc = png::Encoder::new(File::create(&path).unwrap(), size, size);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&paddy_ui::logo::icon_rgba(size as usize)).unwrap();
        println!("{}", path.display());
    }
    std::fs::write(dir.join("paddy.svg"), paddy_ui::logo::icon_svg()).unwrap();
    println!("{}", dir.join("paddy.svg").display());
}
