use paddy_ui::logo;
fn main() {
    println!("{}\n", logo::render(logo::BIG, 0));
    for f in [0, 2, 4, 7] {
        println!("{}\n", logo::render_mini(f));
    }
}
