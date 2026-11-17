use crate::imaging::{reduce_colors, smooth_strays};
use crate::state::Step;
use image::{Rgba, RgbaImage};
use std::path::PathBuf;
use std::time::Duration;

pub struct Palette {
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

fn hex(name: &str, list: &[&str]) -> Palette {
    let colors = list
        .iter()
        .map(|h| {
            let v = u32::from_str_radix(h, 16).unwrap_or(0);
            [(v >> 16) as u8, (v >> 8) as u8, v as u8]
        })
        .collect();
    Palette {
        name: name.to_string(),
        colors,
    }
}

pub fn builtins() -> Vec<Palette> {
    vec![
        hex("Game Boy", &["0f380f", "306230", "8bac0f", "9bbc0f"]),
        hex(
            "PICO-8",
            &[
                "000000", "1d2b53", "7e2553", "008751", "ab5236", "5f574f", "c2c3c7", "fff1e8",
                "ff004d", "ffa300", "ffec27", "00e436", "29adff", "83769c", "ff77a8", "ffccaa",
            ],
        ),
    ]
}

// sRGB -> Oklab, where straight-line distance roughly matches perceived difference.
pub fn to_oklab(c: [u8; 3]) -> [f32; 3] {
    let lin = |v: u8| {
        let x = v as f32 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

// Bayer threshold for any power-of-two size (2, 4, 8), built from the pixel's bits.
// Returns -0.5..+0.5.
fn bayer(x: u32, y: u32, size: u32) -> f32 {
    let mut v = 0u32;
    for i in 0..size.trailing_zeros() {
        let q = match ((x >> i) & 1, (y >> i) & 1) {
            (0, 0) => 0,
            (1, 0) => 2,
            (0, 1) => 3,
            _ => 1,
        };
        v = v * 4 + q;
    }
    (v as f32 + 0.5) / (size * size) as f32 - 0.5
}

// Floyd-Steinberg error diffusion in Oklab. Each pixel's leftover error
// (wanted color minus chosen color) is spread onto its unvisited neighbors.
fn diffuse(
    src: &RgbaImage,
    pal: &Palette,
    pal_lab: &[[f32; 3]],
    hue: f32,
    strength: f32,
) -> RgbaImage {
    let (w, h) = src.dimensions();
    let mut out = src.clone();
    let mut err = vec![[0f32; 3]; (w * h) as usize];
    for y in 0..h {
        let rev = y % 2 == 1; // odd rows run right to left
        for step in 0..w {
            let x = if rev { w - 1 - step } else { step };
            let p = src.get_pixel(x, y).0;
            if p[3] == 0 {
                continue;
            }
            let i = (y * w + x) as usize;
            let base = to_oklab([p[0], p[1], p[2]]);
            let want = [
                base[0] + err[i][0],
                base[1] + err[i][1],
                base[2] + err[i][2],
            ];

            let mut best = 0;
            let mut best_d = f32::MAX;
            for (n, q) in pal_lab.iter().enumerate() {
                let dl = q[0] - want[0];
                let da = q[1] - want[1];
                let db = q[2] - want[2];
                let d = dl * dl + hue * (da * da + db * db);
                if d < best_d {
                    best_d = d;
                    best = n;
                }
            }

            let c = pal.colors[best];
            out.put_pixel(x, y, Rgba([c[0], c[1], c[2], p[3]]));

            let e = [
                (want[0] - pal_lab[best][0]) * strength,
                (want[1] - pal_lab[best][1]) * strength,
                (want[2] - pal_lab[best][2]) * strength,
            ];
            let mut spread = |dx: i32, dy: i32, f: f32| {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    return;
                }
                if src.get_pixel(nx as u32, ny as u32)[3] == 0 {
                    return;
                }
                let j = (ny as u32 * w + nx as u32) as usize;
                for k in 0..3 {
                    err[j][k] += e[k] * f;
                }
            };
            let d = if rev { -1 } else { 1 }; // "ahead" in the scan direction
            spread(d, 0, 7.0 / 16.0);
            spread(-d, 1, 3.0 / 16.0);
            spread(0, 1, 5.0 / 16.0);
            spread(d, 1, 1.0 / 16.0);
        }
    }
    out
}

// Replace every opaque pixel with the nearest palette color (Oklab distance).
// dither: 0 = off, up to 1 = strong Bayer pattern.
pub fn map_to_palette(
    src: &RgbaImage,
    pal: &Palette,
    hue: f32,
    dither: f32,
    mode: u8,
) -> RgbaImage {
    let mut out = src.clone();
    if pal.colors.is_empty() {
        return out;
    }
    // convert the palette once, not once per pixel
    let pal_lab: Vec<[f32; 3]> = pal.colors.iter().map(|&c| to_oklab(c)).collect();
    if mode == 4 {
        return diffuse(src, pal, &pal_lab, hue, dither);
    }
    for (x, y, p) in out.enumerate_pixels_mut() {
        if p[3] == 0 {
            continue;
        }
        let mut lab = to_oklab([p[0], p[1], p[2]]);
        // threshold for this screen position, from -0.5 to +0.5
        let size = match mode {
            1 => 2,
            2 => 4,
            _ => 8,
        };
        if mode > 0 {
            lab[0] += bayer(x, y, size) * dither * 0.5; // nudge lightness before matching
        }
        let dist = |q: &[f32; 3]| {
            let dl = q[0] - lab[0];
            let da = q[1] - lab[1];
            let db = q[2] - lab[2];
            dl * dl + hue * (da * da + db * db) // hue = how much color counts vs lightness
        };
        let mut best = 0;
        let mut best_d = f32::MAX;
        for (i, q) in pal_lab.iter().enumerate() {
            let d = dist(q);
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        let c = pal.colors[best];
        p[0] = c[0];
        p[1] = c[1];
        p[2] = c[2];
    }
    out
}

// One hex color per line (with or without '#'); anything else is skipped.
pub fn parse_hex(text: &str) -> Vec<[u8; 3]> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('#'))
        .filter(|l| l.len() == 6)
        .filter_map(|l| u32::from_str_radix(l, 16).ok())
        .map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
        .collect()
}

pub fn fetch_lospec(slug: &str) -> Result<Palette, String> {
    let slug = slug.trim().to_lowercase();
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Enter a Lospec slug, like pico-8 or endesga-32.".into());
    }
    let url = format!("https://lospec.com/palette-list/{slug}.hex");
    let body = ureq::get(&url)
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| format!("Couldn't fetch '{slug}': {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let colors = parse_hex(&body);
    if colors.is_empty() {
        return Err(format!("'{slug}' had no colors."));
    }
    Ok(Palette { name: slug, colors })
}

pub fn run_recipe(img: RgbaImage, steps: &[Step], pals: &[Palette]) -> RgbaImage {
    let mut img = img;
    for s in steps {
        img = match s {
            Step::Clean(t) => reduce_colors(&img, *t),
            Step::Map(name, hw, dither, mode) => match pals.iter().find(|p| &p.name == name) {
                Some(p) => map_to_palette(&img, p, *hw, *dither, *mode),
                None => img,
            },
            Step::Smooth(n, p) => smooth_strays(&img, *n, *p),
        };
    }
    img
}

fn palette_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("spritesnap").join("palettes"))
}

// Open the palettes folder in the system file manager.
pub fn open_palette_dir() -> Result<(), String> {
    let dir = palette_dir().ok_or("Couldn't find the config folder.")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?; // make sure it exists
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open folder: {e}"))
}

pub fn save_palette(p: &Palette) {
    if let Some(dir) = palette_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let text: String = p
            .colors
            .iter()
            .map(|c| format!("{:02x}{:02x}{:02x}\n", c[0], c[1], c[2]))
            .collect();
        let _ = std::fs::write(dir.join(format!("{}.hex", p.name)), text);
    }
}

pub fn clean_name(s: &str) -> String {
    s.replace(['/', '&', '\\'], "-")
}

pub fn load_saved() -> Vec<Palette> {
    let mut out = Vec::new();
    let Some(dir) = palette_dir() else {
        return out;
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e == "hex"))
        .collect();
    files.sort();
    for f in files {
        if let Ok(text) = std::fs::read_to_string(&f) {
            let colors = parse_hex(&text);
            if !colors.is_empty() {
                let name = f
                .file_stem()
                .map(|s| clean_name(&s.to_string_lossy()))
                .unwrap_or_else(|| "saved".into());
                out.push(Palette { name, colors });
            }
        }
    }
    out
}
