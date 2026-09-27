//! The paddy logo: a floppy disk, drawn from shape geometry (proportions taken
//! from the source image) and packed into half-block characters so it stays
//! crisp at any size. Also drives the little "writing" animation.
//!
//! Everything here is pure and cheap; animation frames are computed on demand
//! only while an animation is playing.

/// Number of frames in the write animation (frame 0 is the idle logo).
pub const FRAMES: usize = 8;

/// Big logo size in half-block cells: `cols` wide, `rows` tall (2 pixels per row).
pub const BIG: (usize, usize) = (24, 11);

/// Header-sized logo, hand-drawn on a 8x4 pixel grid (2 rows of half blocks).
/// The label cells (row 1, columns 2..6) are what the animation fills.
const MINI_TOP: &str = "██▄▄█▄██";
const MINI_BOTTOM: [char; 8] = ['█', '█', '▄', '▄', '▄', '▄', '█', '█'];

fn in_rect(u: f32, v: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> bool {
    u >= x0 && u < x1 && v >= y0 && v < y1
}

/// Rounded-rectangle test in normalized units (`r` is the corner radius).
fn in_round_rect(u: f32, v: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    if !in_rect(u, v, x0, y0, x1, y1) {
        return false;
    }
    // Nearest corner centre when inside a corner square; otherwise the point is in the body.
    let cx = if u < x0 + r {
        x0 + r
    } else if u >= x1 - r {
        x1 - r
    } else {
        return true;
    };
    let cy = if v < y0 + r {
        y0 + r
    } else if v >= y1 - r {
        y1 - r
    } else {
        return true;
    };
    (u - cx).powi(2) + (v - cy).powi(2) <= r * r
}

/// Is the point `(u, v)` (both 0..1) part of the floppy? `p` in 0..=1 is the
/// write-animation progress: it draws text lines onto the label.
fn floppy(u: f32, v: f32, p: f32) -> bool {
    // Body with rounded corners.
    if !in_round_rect(u, v, 0.0, 0.0, 1.0, 1.0, 0.07) {
        return false;
    }
    // Clipped top-right corner.
    if u > 0.87 && v < (u - 0.87) / 0.13 * 0.134 {
        return false;
    }
    // Shutter opening at the top, with the sliding shutter piece inside it.
    if in_rect(u, v, 0.267, 0.0, 0.733, 0.33) {
        return in_rect(u, v, 0.558, 0.035, 0.686, 0.30);
    }
    // Label at the bottom: empty, except for the "text lines" while writing.
    if in_round_rect(u, v, 0.202, 0.55, 0.798, 0.92, 0.04) {
        if p <= 0.0 {
            return false;
        }
        let len = 0.24 + 0.50 * p;
        let line = |y: f32| v >= y && v < y + 0.06;
        return (line(0.62) || line(0.72) || line(0.82)) && u >= 0.25 && u < 0.25 + len - 0.25;
    }
    true
}

/// Render the logo at `cols` x `rows` characters for animation `frame`.
pub fn render(size: (usize, usize), frame: usize) -> String {
    let (cols, rows) = size;
    let p = if frame == 0 { 0.0 } else { (frame as f32 / (FRAMES - 1) as f32).min(1.0) };
    let (pw, ph) = (cols, rows * 2);
    let px = |x: usize, y: usize| floppy((x as f32 + 0.5) / pw as f32, (y as f32 + 0.5) / ph as f32, p);
    let mut out = String::with_capacity((cols + 1) * rows * 3);
    for r in 0..rows {
        for c in 0..cols {
            out.push(match (px(c, r * 2), px(c, r * 2 + 1)) {
                (false, false) => ' ',
                (true, false) => '▀',
                (false, true) => '▄',
                (true, true) => '█',
            });
        }
        if r + 1 < rows {
            out.push('\n');
        }
    }
    out
}

/// Header logo for animation `frame` (0 = idle): the label fills with ink, left to right.
pub fn render_mini(frame: usize) -> String {
    let filled = if frame == 0 { 0 } else { (frame * 4).div_ceil(FRAMES - 1).min(4) };
    let mut bottom = MINI_BOTTOM;
    for cell in bottom.iter_mut().skip(2).take(filled) {
        *cell = '▒';
    }
    format!("{MINI_TOP}\n{}", bottom.iter().collect::<String>())
}

/// Pixel size of the big vector logo (one unit = one block of the ASCII art).
const BIG_PX: (usize, usize) = (BIG.0, BIG.1 * 2);

/// Frame `frame` of the big logo as a pixel grid (`BIG_PX`).
fn big_grid(frame: usize) -> Vec<Vec<bool>> {
    let p = if frame == 0 { 0.0 } else { (frame as f32 / (FRAMES - 1) as f32).min(1.0) };
    let (w, h) = BIG_PX;
    (0..h)
        .map(|y| (0..w).map(|x| floppy((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32, p)).collect())
        .collect()
}

/// The mini logo as a pixel grid, decoded from the half-block art (ink counts as filled).
fn mini_grid(frame: usize) -> Vec<Vec<bool>> {
    let art = render_mini(frame);
    let mut grid = Vec::new();
    for line in art.lines() {
        let (mut top, mut bottom) = (Vec::new(), Vec::new());
        for c in line.chars() {
            let (t, b) = match c {
                '█' | '▒' => (true, true),
                '▀' => (true, false),
                '▄' => (false, true),
                _ => (false, false),
            };
            top.push(t);
            bottom.push(b);
        }
        grid.push(top);
        grid.push(bottom);
    }
    grid
}

/// SVG path data (in pixel-grid units) for a grid: horizontal runs, merged
/// vertically when identical, so it draws as a few crisp rectangles.
fn grid_to_path(grid: &[Vec<bool>]) -> String {
    // (x, width, y_start, height)
    let mut open: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut done: Vec<(usize, usize, usize, usize)> = Vec::new();
    for (y, row) in grid.iter().enumerate() {
        let mut runs = Vec::new();
        let mut x = 0;
        while x < row.len() {
            if row[x] {
                let start = x;
                while x < row.len() && row[x] {
                    x += 1;
                }
                runs.push((start, x - start));
            } else {
                x += 1;
            }
        }
        let mut next = Vec::new();
        for (x, w) in runs {
            match open.iter().position(|&(ox, ow, _, _)| ox == x && ow == w) {
                Some(i) => {
                    let mut r = open.remove(i);
                    r.3 += 1;
                    next.push(r);
                }
                None => next.push((x, w, y, 1)),
            }
        }
        done.append(&mut open);
        open = next;
    }
    done.extend(open);
    done.iter().map(|&(x, w, y, h)| format!("M{x} {y}h{w}v{h}h-{w}z")).collect::<Vec<_>>().join("")
}

/// Big logo as vector path data, viewbox `BIG_PX`.
pub fn big_path(frame: usize) -> String {
    grid_to_path(&big_grid(frame))
}

/// Header logo as vector path data, viewbox `MINI_PX`.
pub fn mini_path(frame: usize) -> String {
    grid_to_path(&mini_grid(frame))
}

/// App icon at `size` px: the floppy in the accent green on a dark rounded tile,
/// anti-aliased (4x4 samples per pixel). RGBA, row-major, straight alpha.
pub fn icon_rgba(size: usize) -> Vec<u8> {
    const TILE: [f32; 3] = [0x0e as f32, 0x10 as f32, 0x0f as f32];
    const INK: [f32; 3] = [0x8c as f32, 0xaa as f32, 0x89 as f32];
    const SS: usize = 4;
    let (margin, radius) = (0.17, 0.22);
    let in_tile = |u: f32, v: f32| {
        let cx = u.clamp(radius, 1.0 - radius);
        let cy = v.clamp(radius, 1.0 - radius);
        (u - cx).powi(2) + (v - cy).powi(2) <= radius * radius
    };
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let (mut tile, mut ink) = (0usize, 0usize);
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32;
                    if in_tile(u, v) {
                        tile += 1;
                        let (fu, fv) = ((u - margin) / (1.0 - 2.0 * margin), (v - margin) / (1.0 - 2.0 * margin));
                        if (0.0..1.0).contains(&fu) && (0.0..1.0).contains(&fv) && floppy(fu, fv, 0.0) {
                            ink += 1;
                        }
                    }
                }
            }
            let a = tile as f32 / (SS * SS) as f32;
            let k = if tile == 0 { 0.0 } else { ink as f32 / tile as f32 };
            for c in 0..3 {
                out.push((TILE[c] + (INK[c] - TILE[c]) * k).round() as u8);
            }
            out.push((a * 255.0).round() as u8);
        }
    }
    out
}

/// The same icon as SVG, drawn with smooth shapes that mirror `floppy()`'s geometry
/// (floppy scaled into a 66-unit box, 17 units in from each edge of a 100x100 tile).
pub fn icon_svg() -> String {
    concat!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">"##,
        r##"<rect width="100" height="100" rx="22" fill="#0e100f"/>"##,
        // body with the shutter notch and clipped corner; the label is a hole (evenodd)
        r##"<path fill="#8caa89" fill-rule="evenodd" d="M21.62 17H34.62V38.78H65.38V17H74.42L83 25.84V78.38A4.62 4.62 0 0 1 78.38 83H21.62A4.62 4.62 0 0 1 17 78.38V21.62A4.62 4.62 0 0 1 21.62 17Z"##,
        r##"M32.97 53.3H67.03A2.64 2.64 0 0 1 69.67 55.94V75.08A2.64 2.64 0 0 1 67.03 77.72H32.97A2.64 2.64 0 0 1 30.33 75.08V55.94A2.64 2.64 0 0 1 32.97 53.3Z"/>"##,
        // the sliding shutter piece
        r##"<rect x="53.83" y="19.31" width="8.45" height="17.49" rx="1.3" fill="#8caa89"/>"##,
        "</svg>\n"
    )
    .to_string()
}

#[cfg(test)]
fn solid(x: usize, y: usize, size: usize) -> bool {
    floppy((x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINI_PX: (usize, usize) = (8, 4);

    #[test]
    fn idle_logo_has_the_floppy_features() {
        let big = render(BIG, 0);
        let lines: Vec<&str> = big.lines().collect();
        assert_eq!(lines.len(), BIG.1);
        assert!(lines.iter().all(|l| l.chars().count() == BIG.0));
        // solid body, empty label window near the bottom, shutter notch at the top
        assert!(lines[5].chars().all(|c| c == '█'), "solid band between shutter and label");
        assert!(lines[8].contains(' '), "label window is open");
        assert!(lines[0].contains(' ') && lines[0].contains('█'), "notch cut into the top edge");
    }

    #[test]
    fn writing_animation_adds_ink_then_returns_to_idle() {
        let ink = |f| render(BIG, f).chars().filter(|&c| c != ' ').count();
        let idle = ink(0);
        assert!(ink(1) > idle, "first frame already draws something");
        assert!(ink(FRAMES - 1) > ink(1), "progress grows");
        assert_eq!(render(BIG, 0), render(BIG, 0), "idle is deterministic");
        assert_ne!(render(BIG, 3), render(BIG, 0));
    }

    #[test]
    fn mini_logo_animates_and_returns_to_idle() {
        let idle = render_mini(0);
        assert_eq!(idle.lines().count(), 2);
        assert!(idle.lines().all(|l| l.chars().count() == 8));
        assert!(!idle.contains('▒'));
        assert!(render_mini(1).contains('▒'));
        assert_eq!(render_mini(FRAMES - 1).matches('▒').count(), 4, "label fully inked at the end");
        let ink = |f| render_mini(f).matches('▒').count();
        assert!((1..FRAMES).all(|f| ink(f) >= ink(f - 1)), "never un-inks mid-animation");
    }

    /// Fill a grid from path data ("M x y h w v h h -w z" rectangles) to compare with the source grid.
    fn rasterize(path: &str, w: usize, h: usize) -> Vec<Vec<bool>> {
        let mut grid = vec![vec![false; w]; h];
        for rect in path.split('M').filter(|r| !r.is_empty()) {
            let (xy, rest) = rect.split_once('h').unwrap();
            let (x, y) = xy.split_once(' ').unwrap();
            let (rw, rest) = rest.split_once('v').unwrap();
            let (rh, _) = rest.split_once('h').unwrap();
            let (x, y, rw, rh): (usize, usize, usize, usize) =
                (x.parse().unwrap(), y.parse().unwrap(), rw.parse().unwrap(), rh.parse().unwrap());
            for row in grid.iter_mut().skip(y).take(rh) {
                for cell in row.iter_mut().skip(x).take(rw) {
                    assert!(!*cell, "rectangles must not overlap");
                    *cell = true;
                }
            }
        }
        grid
    }

    #[test]
    fn vector_paths_match_the_pixel_grids() {
        for f in 0..FRAMES {
            assert_eq!(rasterize(&big_path(f), BIG_PX.0, BIG_PX.1), big_grid(f), "big frame {f}");
            assert_eq!(rasterize(&mini_path(f), MINI_PX.0, MINI_PX.1), mini_grid(f), "mini frame {f}");
        }
        assert!(big_path(0).len() < 900, "few rectangles, not one per pixel: {}", big_path(0).len());
        assert_ne!(big_path(0), big_path(3));
        assert_ne!(mini_path(0), mini_path(3));
    }

    #[test]
    fn app_icon_is_a_rounded_tile_with_the_floppy() {
        for size in [16, 32, 256] {
            let px = icon_rgba(size);
            assert_eq!(px.len(), size * size * 4);
            let at = |x: usize, y: usize| &px[(y * size + x) * 4..(y * size + x) * 4 + 4];
            assert_eq!(at(0, 0)[3], 0, "{size}: rounded corner is transparent");
            assert_eq!(at(size / 2, size / 8)[3], 255, "{size}: tile is opaque");
            let green = |p: &[u8]| p[1] > 150 && p[0] > 100;
            assert!((0..size * size).any(|i| green(&px[i * 4..i * 4 + 4])), "{size}: floppy is drawn");
        }
        let svg = icon_svg();
        assert!(
            svg.starts_with("<svg")
                && svg.contains("evenodd")
                && svg.contains("#8caa89")
                && svg.trim_end().ends_with("</svg>")
        );
    }

    #[test]
    fn icon_mask_is_mostly_solid() {
        let n = 32;
        let solid_px = (0..n * n).filter(|i| solid(i % n, i / n, n)).count();
        assert!(solid_px * 100 / (n * n) > 55 && solid_px * 100 / (n * n) < 90, "{solid_px}");
        assert!(!solid(0, 0, n), "rounded corner is empty");
        assert!(solid(1, n / 2, n) && solid(n / 2, n / 2, n), "middle band is solid");
    }
}
