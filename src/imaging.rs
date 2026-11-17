use crate::palette::to_oklab;
use image::{Rgba, RgbaImage};
use std::collections::HashMap;

// Oklab distances are tiny (black to white = 1.0), so scale them to look like the old
// 0-442 RGB units. That way the Merge Tol spinner still feels familiar.
const LAB_SCALE: f32 = 442.0;

fn lab_dist(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt() * LAB_SCALE
}

// Edge strength between neighbouring columns (horiz=true) or rows (false).
fn profile(src: &RgbaImage, horiz: bool) -> Vec<f32> {
    let (w, h) = src.dimensions();
    let (n, m) = if horiz { (w, h) } else { (h, w) };
    let mut out = vec![0f32; n.saturating_sub(1) as usize];
    for i in 0..n.saturating_sub(1) {
        let mut s = 0u32;
        for j in 0..m {
            let (a, b) = if horiz {
                (src.get_pixel(i, j), src.get_pixel(i + 1, j))
            } else {
                (src.get_pixel(j, i), src.get_pixel(j, i + 1))
            };
            for k in 0..4 {
                s += (a[k] as i32 - b[k] as i32).unsigned_abs();
            }
        }
        out[i as usize] = s as f32;
    }
    out
}

// Prevent grid cuts from clipping tiny 1-2 pixel outer features (like flower petals or leaf tips)
fn protect_slivers(src: &RgbaImage, cuts: &mut Vec<u32>, horiz: bool) {
    if cuts.len() < 3 {
        return;
    }
    let (w, h) = src.dimensions();
    let max_len = if horiz { w } else { h };

    for i in 1..cuts.len() - 1 {
        let cut_pos = cuts[i];
        let next_cut = cuts[i + 1];

        // Only nudge if there's room before the next cut (prevents zero-width cells)
        if cut_pos > 1 && cut_pos + 1 < next_cut && cut_pos < max_len - 1 {
            let check_slice = |pos: u32| {
                let total = if horiz { h } else { w };
                (0..total).any(|j| {
                    let p = if horiz {
                        src.get_pixel(pos, j)
                    } else {
                        src.get_pixel(j, pos)
                    };
                    p[3] > 0
                })
            };

            let has_inner_content = check_slice(cut_pos - 1);
            let has_outer_sliver = check_slice(cut_pos) && !check_slice(cut_pos + 1);

            if has_inner_content && has_outer_sliver {
                cuts[i] = cut_pos + 1;
            }
        }
    }
}

// Find where the pixel lattice actually starts (0..s). After a crop the first true
// edge is no longer at 0, so pick the offset whose evenly spaced positions hit the strongest edges.
fn find_phase(p: &[f32], len: u32, s: f32) -> f32 {
    let steps = ((s * 4.0) as u32).max(1); // quarter-pixel resolution
    let mut best = 0.0f32;
    let mut best_score = f32::MIN;
    for i in 0..steps {
        let f = i as f32 / 4.0;
        let (mut sum, mut n) = (0.0f32, 0u32);
        let mut pos = f;
        while pos < len as f32 - 1.0 {
            let c = pos.round() as usize;
            if c >= 1 {
                sum += p.get(c - 1).copied().unwrap_or(0.0);
                n += 1;
            }
            pos += s;
        }
        let score = if n > 0 { sum / n as f32 } else { 0.0 };
        if score > best_score {
            best_score = score;
            best = f;
        }
    }
    best
}

//Greedy windowed walk: place each cut at the strongest edge near pos + s, never revisiting a position
fn find_cuts(p: &[f32], len: u32, s: f32) -> Vec<u32> {
    if len == 0 || s < 1.0 {
        return vec![0, len];
    }
    let mut cuts = vec![0u32];
    let mut pos = 0.0f32;
    let phase = find_phase(p, len, s);
    if phase >= 0.5 * s {
        let c = phase.round() as u32;
        if c > 0 && c < len {
            cuts.push(c);
            pos = c as f32;
        }
    } else {
        pos = phase;
    }

    let min_step = (0.5 * s).max(1.0);
    let max_step = (1.5 * s).max(min_step + 1.0);
    while pos < len as f32 - 1.0 {
        let last_cut = *cuts.last().unwrap();
        let ideal = pos + s;
        let search_radius = 0.4 * s;
        // Strictly > last_cut so pos always advances and duplicate cuts are impossible
        let lo = ((ideal - search_radius).max(pos + min_step) as u32)
            .max(last_cut + 1)
            .min(len - 1);
        let hi = ((ideal + search_radius).min(pos + max_step) as u32)
            .max(lo)
            .min(len - 1);

        if lo >= len - 1 && last_cut >= len - 1 {
            break;
        }

        let mut best_pos = lo;
        let mut best_score = f32::MIN;

        for c in lo..=hi {
            let e = p
                .get((c.saturating_sub(1)) as usize)
                .copied()
                .unwrap_or(0.0);
            let d = (c as f32 - ideal).abs();
            let score = e - 0.05 * (d * d);

            if score > best_score {
                best_score = score;
                best_pos = c;
            }
        }

        cuts.push(best_pos);
        pos = best_pos as f32;
    }

    let last = *cuts.last().unwrap();
    if last < len {
        if (len - last) as f32 >= 0.5 * s || cuts.len() == 1 {
            cuts.push(len);
        } else {
            *cuts.last_mut().unwrap() = len;
        }
    }
    cuts
}

// Row-edge strength, sampled at the center pixel of every established column cell.
fn profile_conditioned(src: &RgbaImage, xs: &[u32]) -> Vec<f32> {
    let (w, h) = src.dimensions();
    let mut out = vec![0f32; h.saturating_sub(1) as usize];
    for i in 0..h.saturating_sub(1) {
        let (mut s, mut n) = (0u32, 0u32);
        for pair in xs.windows(2) {
            let mid_x = (pair[0] + pair[1]) / 2;
            if mid_x < w {
                let (a, b) = (src.get_pixel(mid_x, i), src.get_pixel(mid_x, i + 1));
                for k in 0..4 {
                    s += (a[k] as i32 - b[k] as i32).unsigned_abs();
                }
                n += 1;
            }
        }
        out[i as usize] = if n > 0 { s as f32 / n as f32 } else { 0.0 };
    }
    out
}

// Two-step dependent cut detection: finds horizontal cuts first, then conditions vertical cuts on them
// with sliver protection for delicate features.
pub fn cuts(src: &RgbaImage, block: f32) -> (Vec<u32>, Vec<u32>) {
    let s = block.max(1.0);
    // Step 1: Find horizontal cuts (columns)
    let mut xs = find_cuts(&profile(src, true), src.width(), s);
    protect_slivers(src, &mut xs, true);

    // Step 2: Find vertical cuts (rows) conditioned on the horizontal cell structure
    let mut ys = find_cuts(&profile_conditioned(src, &xs), src.height(), s);
    protect_slivers(src, &mut ys, false);

    (xs, ys)
}

// Shrink each block x block cell into ONE pixel.
// Colors are bucketed (to ignore AI noise); the biggest bucket wins,
// and we average the real colors inside it.
// One output pixel per cell; cells follow the detected borders.
pub fn downscale(src: &RgbaImage, block: f32) -> RgbaImage {
    let (xs, ys) = cuts(src, block);
    let (w, h) = ((xs.len() - 1).max(1) as u32, (ys.len() - 1).max(1) as u32);
    let mut out = RgbaImage::new(w, h);
    for gy in 0..h as usize {
        for gx in 0..w as usize {
            let (x0, x1) = (xs[gx], xs[gx + 1]);
            let (y0, y1) = (ys[gy], ys[gy + 1]);
            let (mx, my) = (x1.saturating_sub(x0) / 4, y1.saturating_sub(y0) / 4); // skip noisy outer quarter
            let mut buckets: HashMap<(u8, u8, u8, u8), (u32, u32, u32, u32, u32)> = HashMap::new();
            for y in (y0 + my)..(y1 - my) {
                for x in (x0 + mx)..(x1 - mx) {
                    let p = src.get_pixel(x, y).0;
                    let key = (p[0] >> 3, p[1] >> 3, p[2] >> 3, p[3] >> 6);
                    let e = buckets.entry(key).or_insert((0, 0, 0, 0, 0));
                    e.0 += 1;
                    e.1 += p[0] as u32;
                    e.2 += p[1] as u32;
                    e.3 += p[2] as u32;
                    e.4 += p[3] as u32;
                }
            }
            if let Some((_, best)) = buckets.iter().max_by_key(|(k, v)| (v.0, **k)) {
                let n = best.0;
                out.put_pixel(
                    gx as u32,
                    gy as u32,
                    image::Rgba([
                        (best.1 / n) as u8,
                        (best.2 / n) as u8,
                        (best.3 / n) as u8,
                        if best.4 / n >= 128 { 255 } else { 0 },
                    ]),
                );
            } else if x0 < src.width() && y0 < src.height() {
                let p = *src.get_pixel((x0 + x1) / 2, (y0 + y1) / 2);
                out.put_pixel(
                    gx as u32,
                    gy as u32,
                    image::Rgba([p[0], p[1], p[2], if p[3] >= 128 { 255 } else { 0 }]),
                );
            }
        }
    }
    out
}

// Flood-fill from the image border; anything within `tol` of the
// background color (the average of the 4 corners) becomes transparent.
pub fn remove_bg(src: &RgbaImage, tol: f32) -> RgbaImage {
    let (w, h) = src.dimensions();
    if w == 0 || h == 0 {
        return src.clone();
    }
    let mut out = src.clone();

    let mut bg = [0f32; 3];
    for &(x, y) in &[(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
        let p = src.get_pixel(x, y).0;
        for i in 0..3 {
            bg[i] += p[i] as f32 / 4.0;
        }
    }
    let close = |p: [u8; 4]| {
        let d = (p[0] as f32 - bg[0]).powi(2)
            + (p[1] as f32 - bg[1]).powi(2)
            + (p[2] as f32 - bg[2]).powi(2);
        d.sqrt() <= tol
    };

    let mut seen = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }

    while let Some((x, y)) = stack.pop() {
        let i = (y * w + x) as usize;
        if seen[i] {
            continue;
        }
        seen[i] = true;
        if !close(src.get_pixel(x, y).0) {
            continue;
        }
        out.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    out
}

// Bounding box (x, y, w, h) of all non-transparent pixels.
pub fn content_bounds(img: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] > 0 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 == u32::MAX {
        None
    } else {
        Some((x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    }
}

// Estimate the fake-pixel size from the periodic spacing of edge spikes.
pub fn detect_size(img: &RgbaImage) -> Option<f32> {
    const MIN_D: usize = 3;
    const MAX_D: usize = 64;
    // scale each profile to 0..1 and remove its mean
    let prep = |v: Vec<f32>| {
        let m = v.iter().cloned().fold(0.0f32, f32::max).max(1.0);
        let mean = v.iter().sum::<f32>() / v.len().max(1) as f32 / m;
        v.iter().map(|x| x / m - mean).collect::<Vec<f32>>()
    };
    let sigs = [prep(profile(img, true)), prep(profile(img, false))];

    // autocorrelation: how well the signal matches itself shifted by d
    let mut ac = vec![0f32; MAX_D + 2];
    for d in 1..=MAX_D + 1 {
        for s in &sigs {
            if s.len() > d {
                let n = s.len() - d;
                ac[d] += (0..n).map(|i| s[i] * s[i + d]).sum::<f32>() / n as f32;
            }
        }
    }

    let top = ac[MIN_D..=MAX_D].iter().cloned().fold(f32::MIN, f32::max);
    if top <= 0.0 {
        return None;
    }

    // first local peak that is nearly as strong as the best (avoids 2x/3x multiples)
    for d in MIN_D..=MAX_D {
        if ac[d] >= 0.6 * top && ac[d] >= ac[d - 1] && ac[d] >= ac[d + 1] {
            let (a, b, c) = (ac[d - 1], ac[d], ac[d + 1]);
            let den = a - 2.0 * b + c;
            let off = if den.abs() > 1e-9 {
                0.5 * (a - c) / den
            } else {
                0.0
            };
            return Some(d as f32 + off.clamp(-0.5, 0.5)); // parabola fit = fractional size
        }
    }
    None
}

// Merge colors within `tol` (RGB distance) of a more common color.
pub fn reduce_colors(src: &RgbaImage, tol: f32) -> RgbaImage {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for p in src.pixels() {
        if p[3] > 0 {
            *counts.entry([p[0], p[1], p[2]]).or_insert(0) += 1;
        }
    }
    let mut colors: Vec<([u8; 3], u32)> = counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0))); // most common first

    // each kept color stores its Oklab value so we only convert it once
    let mut palette: Vec<([u8; 3], [f32; 3])> = Vec::new();
    let mut map: HashMap<[u8; 3], [u8; 3]> = HashMap::new();
    for (c, _) in &colors {
        let lab = to_oklab(*c);
        let near = palette
            .iter()
            .map(|(q, ql)| (lab_dist(&lab, ql), *q))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        match near {
            Some((d, q)) if d <= tol => {
                map.insert(*c, q);
            }
            _ => {
                palette.push((*c, lab));
                map.insert(*c, *c);
            }
        }
    }

    let mut out = src.clone();
    for p in out.pixels_mut() {
        if p[3] > 0 {
            let q = map[&[p[0], p[1], p[2]]];
            p[0] = q[0];
            p[1] = q[1];
            p[2] = q[2];
        }
    }
    out
}

// How many colors would remain at this tolerance (same logic as reduce_colors, no image built).
pub fn predict_colors(src: &RgbaImage, tol: f32) -> usize {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for p in src.pixels() {
        if p[3] > 0 {
            *counts.entry([p[0], p[1], p[2]]).or_insert(0) += 1;
        }
    }
    let mut colors: Vec<([u8; 3], u32)> = counts.into_iter().collect();
    colors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut palette: Vec<[f32; 3]> = Vec::new();
    for (c, _) in &colors {
        let lab = to_oklab(*c);
        if !palette.iter().any(|q| lab_dist(&lab, q) <= tol) {
            palette.push(lab);
        }
    }
    palette.len()
}

// Smallest tolerance that gets the palette down to `target` colors or fewer.
pub fn tol_for_target(src: &RgbaImage, target: usize) -> f32 {
    let (mut lo, mut hi) = (0.0f32, 442.0f32);
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        if predict_colors(src, mid) <= target {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

// Replace stray pixels: a pixel with at most `max_same` same-colored neighbors
// (of 8) takes the neighborhood's dominant color, if that color has 3+ neighbors.
pub fn smooth_strays(src: &RgbaImage, max_same: usize, protect: f32) -> RgbaImage {
    let (w, h) = src.dimensions();
    let mut out = src.clone();
    for y in 0..h {
        for x in 0..w {
            let p = src.get_pixel(x, y).0;
            if p[3] == 0 {
                continue;
            }
            let mine = [p[0], p[1], p[2]];
            let mut same = 0usize;
            let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let q = src.get_pixel(nx as u32, ny as u32).0;
                    if q[3] == 0 {
                        continue;
                    }
                    let c = [q[0], q[1], q[2]];
                    if c == mine {
                        same += 1;
                    } else {
                        *counts.entry(c).or_insert(0) += 1;
                    }
                }
            }
            if same > max_same {
                continue;
            }
            if let Some((c, n)) = counts.into_iter().max_by_key(|(c, n)| (*n, *c)) {
                let dist = {
                    let d = |i: usize| (c[i] as f32 - mine[i] as f32).powi(2);
                    (d(0) + d(1) + d(2)).sqrt()
                };
                if n >= 3 && n > same && dist <= protect {
                    out.put_pixel(x, y, Rgba([c[0], c[1], c[2], p[3]]));
                }
            }
        }
    }
    out
}
