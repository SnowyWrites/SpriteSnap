use crate::imaging::{cuts, predict_colors};
use crate::state::*;
use fltk::{
    button::CheckButton, enums::ColorDepth, frame::Frame, image::RgbImage, misc::Spinner,
    prelude::*,
};
use image::RgbaImage;
use std::{
    cell::{Cell, RefCell},
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    rc::Rc,
};

pub const PREVIEW_W: u32 = 700;
pub const PREVIEW_H: u32 = 500;

// Grid lines in image pixels: (x positions, y positions).
pub type Lines = (Vec<f32>, Vec<f32>);

pub fn uniform(w: u32, h: u32) -> Lines {
    (
        (0..=w).map(|x| x as f32).collect(),
        (0..=h).map(|y| y as f32).collect(),
    )
}

#[derive(Clone)]
pub struct View {
    pub frame: Frame,
    pub info: Frame,
    pub grid: CheckButton,
    pub block: Spinner,
    pub source: Slot,
    pub result: Slot,
    pub crop: CropSlot,
    pub snap_block: Rc<Cell<f32>>,
    pub vp: Rc<Cell<Zoom>>,
    pub cache: Rc<RefCell<Option<(u64, Vec<u32>, Vec<u32>)>>>,
    pub merge: Spinner,
    pub predict: Frame,
    pub hover: Rc<Cell<Option<(i32, i32)>>>,
    pub tool: Rc<Cell<u8>>,
}

impl View {
    // Cuts for the source at this block size, cached so panning/dragging stays smooth.
    fn source_lines(&self, s: &RgbaImage, block: f32) -> Lines {
        let mut h = DefaultHasher::new();
        (s.width(), s.height(), block.to_bits()).hash(&mut h);
        for b in s.as_raw().iter().step_by(997) {
            b.hash(&mut h);
        }
        let key = h.finish();
        let mut c = self.cache.borrow_mut();
        if c.as_ref().map(|e| e.0) != Some(key) {
            let (xs, ys) = cuts(s, block);
            *c = Some((key, xs, ys));
        }
        let e = c.as_ref().unwrap();
        (
            e.1.iter().map(|&x| x as f32).collect(),
            e.2.iter().map(|&y| y as f32).collect(),
        )
    }

    pub fn refresh(&mut self) {
        let src = self.source.borrow();
        let res = self.result.borrow();
        let crop = self.crop.borrow();
        let grid_on = self.grid.is_checked();
        let block = (self.block.value() as f32).max(1.0);
        let vp = self.vp.get();
        let hover = if self.tool.get() != 0 && res.is_some() {
            self.hover.get()
        } else {
            None
        };

        let grid: Option<Lines> = if !grid_on {
            None
        } else {
            match (&*res, &*src) {
                (Some(r), _) => Some(uniform(r.width(), r.height())),
                (None, Some(s)) => Some(self.source_lines(s, block)),
                _ => None,
            }
        };

        if crop.active {
            let target = match (&*res, &*src) {
                (Some(r), _) => Some(r),
                (None, Some(s)) => Some(s),
                _ => None,
            };
            if let Some(img) = target {
                show(
                    &mut self.frame,
                    img,
                    grid.as_ref(),
                    Some(crop.rect),
                    vp,
                    None,
                );
                let (x0, y0, x1, y1) = crop.rect;
                self.info.set_label(&format!(
                    "Crop: {}x{} at ({}, {})  |  {}: {}x{}",
                    (x1 - x0).round(),
                    (y1 - y0).round(),
                    x0.round(),
                    y0.round(),
                    if res.is_some() { "Sprite" } else { "Image" },
                    img.width(),
                    img.height()
                ));
                self.info.redraw();
                return;
            }
        }

        match (&*res, &*src) {
            (Some(r), _) => show(&mut self.frame, r, grid.as_ref(), None, vp, hover),
            (None, Some(s)) => show(&mut self.frame, s, grid.as_ref(), None, vp, hover),
            _ => {
                self.frame.set_image(None::<RgbImage>);
                self.frame.redraw();
            }
        }

        let text = match (&*res, &*src) {
            (Some(r), Some(s)) => {
                let n = r
                    .pixels()
                    .filter(|p| p[3] > 0)
                    .map(|p| [p[0], p[1], p[2]])
                    .collect::<HashSet<_>>()
                    .len();
                format!(
                    "Sprite: {}x{}  |  {} colors  |  Source: {}x{} (block {:.1})",
                    r.width(),
                    r.height(),
                    n,
                    s.width(),
                    s.height(),
                    self.snap_block.get()
                )
            }
            (None, Some(s)) => format!(
                "Image: {}x{}  |  Grid at block {:.1}: {}x{} cells",
                s.width(),
                s.height(),
                block,
                (s.width() as f32 / block) as u32,
                (s.height() as f32 / block) as u32
            ),
            _ => String::from("No image loaded"),
        };
        match &*res {
            Some(r) => self.predict.set_label(&format!(
                "Merge {:.0} → {} colors",
                self.merge.value(),
                predict_colors(r, self.merge.value() as f32)
            )),
            None => self.predict.set_label(""),
        }
        self.predict.redraw();
        self.info.set_label(&text);
        self.info.redraw();
    }
}

// zoom
#[derive(Clone, Copy)]
pub struct Zoom {
    pub z: f32,
    pub cx: f32,
    pub cy: f32,
} // z: 1.0 = fit; cx, cy: view center as fraction of image
pub const FIT: Zoom = Zoom {
    z: 1.0,
    cx: 0.5,
    cy: 0.5,
};

// (scale, ox, oy): image pixel (ix, iy) lands at canvas (ix*scale + ox, iy*scale + oy)
pub fn xform(iw: u32, ih: u32, v: Zoom) -> (f32, f32, f32) {
    let (aw, ah) = ((PREVIEW_W - 20) as f32, (PREVIEW_H - 20) as f32);
    let scale = (aw / iw as f32).min(ah / ih as f32) * v.z;
    let ox = PREVIEW_W as f32 / 2.0 - v.cx * iw as f32 * scale;
    let oy = PREVIEW_H as f32 / 2.0 - v.cy * ih as f32 * scale;
    (scale, ox, oy)
}

// Scale an image to fit the preview area using nearest-neighbor (keeps pixels crisp).
// cell = Some(size in image pixels) draws grid lines every `size` pixels.
pub fn show(
    frame: &mut Frame,
    img: &RgbaImage,
    grid: Option<&Lines>,
    crop: Option<(f32, f32, f32, f32)>,
    vp: Zoom,
    cursor: Option<(i32, i32)>,
) {
    let (iw, ih) = (img.width(), img.height());
    let (scale, ox, oy) = xform(iw, ih, vp);
    let (w, h) = (PREVIEW_W as i32, PREVIEW_H as i32);
    let mut canvas = RgbaImage::from_pixel(PREVIEW_W, PREVIEW_H, image::Rgba([70, 70, 90, 255]));

    // for each canvas pixel, look up the image pixel under it (dimming outside the crop box)
    for py in 0..h {
        let fy = (py as f32 - oy) / scale;
        if fy < 0.0 || fy >= ih as f32 {
            continue;
        }
        for px in 0..w {
            let fx = (px as f32 - ox) / scale;
            if fx < 0.0 || fx >= iw as f32 {
                continue;
            }
            let mut p = *img.get_pixel(fx as u32, fy as u32);
            if let Some((x0, y0, x1, y1)) = crop {
                if fx < x0 || fx >= x1 || fy < y0 || fy >= y1 {
                    for i in 0..3 {
                        p[i] = (p[i] as f32 * 0.4) as u8;
                    }
                }
            }
            canvas.put_pixel(px as u32, py as u32, p);
        }
    }

    // grid lines at the given image-pixel positions
    if let Some((gx, gy)) = grid {
        let line = image::Rgba([0, 255, 160, 255]);
        let avg = |v: &Vec<f32>| {
            if v.len() > 1 {
                (v[v.len() - 1] - v[0]) / (v.len() - 1) as f32
            } else {
                0.0
            }
        };
        if avg(gx) * scale >= 2.0 && avg(gy) * scale >= 2.0 {
            let (x_end, y_end) = (ox + iw as f32 * scale, oy + ih as f32 * scale);
            let (ya, yb) = (oy.max(0.0) as i32, y_end.min(h as f32) as i32);
            let (xa, xb) = (ox.max(0.0) as i32, x_end.min(w as f32) as i32);
            for &x in gx {
                let px = ox + x * scale;
                if px >= 0.0 && px < w as f32 {
                    for y in ya..yb {
                        canvas.put_pixel(px as u32, y as u32, line);
                    }
                }
            }
            for &y in gy {
                let py = oy + y * scale;
                if py >= 0.0 && py < h as f32 {
                    for x in xa..xb {
                        canvas.put_pixel(x as u32, py as u32, line);
                    }
                }
            }
        }
    }

    // brush cursor: outline the pixel under the mouse
    if let Some((cx, cy)) = cursor {
        let (x0, y0) = (
            (cx as f32 * scale + ox) as i32,
            (cy as f32 * scale + oy) as i32,
        );
        let (x1, y1) = (
            ((cx + 1) as f32 * scale + ox) as i32,
            ((cy + 1) as f32 * scale + oy) as i32,
        );
        let c = image::Rgba([255, 255, 255, 255]);
        let mut put = |x: i32, y: i32| {
            if x >= 0 && y >= 0 && x < w && y < h {
                canvas.put_pixel(x as u32, y as u32, c);
            }
        };
        for x in x0..=x1 {
            put(x, y0);
            put(x, y1);
        }
        for y in y0..=y1 {
            put(x0, y);
            put(x1, y);
        }
    }

    // crop outline + handles, in canvas coordinates
    if let Some((x0, y0, x1, y1)) = crop {
        let (px0, py0, px1, py1) = (
            (x0 * scale + ox) as i32,
            (y0 * scale + oy) as i32,
            (x1 * scale + ox) as i32,
            (y1 * scale + oy) as i32,
        );
        let c = image::Rgba([255, 220, 0, 255]);
        let mut put = |x: i32, y: i32| {
            if x >= 0 && y >= 0 && x < w && y < h {
                canvas.put_pixel(x as u32, y as u32, c);
            }
        };
        for x in px0..=px1 {
            put(x, py0);
            put(x, py1);
        }
        for y in py0..=py1 {
            put(px0, y);
            put(px1, y);
        }
        let (mx, my) = ((px0 + px1) / 2, (py0 + py1) / 2);
        for &(hx, hy) in &[
            (px0, py0),
            (mx, py0),
            (px1, py0),
            (px0, my),
            (px1, my),
            (px0, py1),
            (mx, py1),
            (px1, py1),
        ] {
            for dy in -4..=4 {
                for dx in -4..=4 {
                    put(hx + dx, hy + dy);
                }
            }
        }
    }

    let rgb = RgbImage::new(
        canvas.as_raw(),
        PREVIEW_W as i32,
        PREVIEW_H as i32,
        ColorDepth::Rgba8,
    )
    .unwrap();
    frame.set_image(Some(rgb));
    frame.redraw();
}
