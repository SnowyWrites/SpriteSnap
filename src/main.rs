#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod imaging;
use imaging::*;
mod state;
use state::*;
mod view;
use view::*;
mod palette;
use palette::*;

use fltk::{
    app,
    button::{Button, CheckButton},
    dialog, draw,
    enums::{Align, Color, Event, FrameType, Key, Shortcut},
    frame::Frame,
    input::Input,
    menu::Choice,
    misc::Spinner,
    prelude::*,
    window::Window,
};
use image::{RgbaImage, imageops};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// Draw a straight line of pixels from a to b (Bresenham), so fast drags leave no gaps.
fn paint_line(img: &mut RgbaImage, a: (i32, i32), b: (i32, i32), px: image::Rgba<u8>) {
    let (mut x, mut y) = a;
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    let (sx, sy) = (
        if a.0 < b.0 { 1 } else { -1 },
        if a.1 < b.1 { 1 } else { -1 },
    );
    let mut err = dx + dy;
    loop {
        if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
            img.put_pixel(x as u32, y as u32, px);
        }
        if x == b.0 && y == b.1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

// Add a color to the "custom" palette (creating it if needed) and select it.
fn add_to_custom(list: &Rc<RefCell<Vec<Palette>>>, choice: &mut Choice, c: [u8; 3]) {
    let idx = list.borrow().iter().position(|q| q.name == "custom");
    match idx {
        Some(i) => {
            let mut l = list.borrow_mut();
            if !l[i].colors.contains(&c) {
                l[i].colors.push(c);
            }
            save_palette(&l[i]);
            choice.set_value(i as i32);
        }
        None => add_palette(
            list,
            choice,
            Palette {
                name: "custom".into(),
                colors: vec![c],
            },
        ),
    }
}

// Append a palette to the list and select it in the dropdown.
fn add_palette(list: &Rc<RefCell<Vec<Palette>>>, choice: &mut Choice, p: Palette) {
    let existing = list.borrow().iter().position(|q| q.name == p.name);
    if let Some(i) = existing {
        choice.set_value(i as i32); // already have it: just select it
        return;
    }
    save_palette(&p);
    choice.add_choice(&p.name.replace('/', "-"));
    let mut l = list.borrow_mut();
    l.push(p);
    choice.set_value(l.len() as i32 - 1);
}

// Returns true if it's OK to continue. Warns first if brush touch-ups would be lost.
fn confirm_discard(painted: &Cell<bool>) -> bool {
    if !painted.get() {
        return true;
    }
    let ok = dialog::choice2_default(
        "This redoes the snap and erases your brush touch-ups.",
        "Cancel",
        "Continue",
        "",
    ) == Some(1);
    if ok {
        painted.set(false);
    }
    ok
}

fn main() {
    let app = app::App::default();
    //widgits aka the buttons
    let mut win = Window::new(100, 100, 1010, 635, "SpriteSnap");

    // app icon
    if let Ok(icon_img) = image::load_from_memory(include_bytes!("assets/icon.png")) {
        let rgba = icon_img.to_rgba8();
        let (w, h) = (rgba.width() as i32, rgba.height() as i32);
        if let Ok(fltk_icon) = fltk::image::RgbImage::new(rgba.as_raw(), w, h, fltk::enums::ColorDepth::Rgba8,) {
            win.set_icon(Some(fltk_icon));
        }
    }

    // ---- top bar (y = 10) ----
    let mut open_btn = Button::new(10, 10, 60, 30, "Open");
    let mut save_btn = Button::new(75, 10, 60, 30, "Save");
    let mut scale_spinner = Spinner::new(190, 10, 60, 30, "Save x:");
    scale_spinner.set_minimum(1.0);
    scale_spinner.set_maximum(32.0);
    scale_spinner.set_step(1.0);
    scale_spinner.set_value(1.0);
    let mut undo_btn = Button::new(270, 10, 50, 30, "Undo");
    let mut redo_btn = Button::new(320, 10, 50, 30, "Redo");
    let mut reset_btn = Button::new(370, 10, 55, 30, "Reset");
    let mut crop_btn = Button::new(450, 10, 105, 30, "Crop to sprite");
    let mut dragcrop_btn = Button::new(555, 10, 80, 30, "Drag crop");
    let mut apply_btn = Button::new(635, 10, 55, 30, "Apply");
    let mut snap_check = CheckButton::new(700, 10, 100, 30, "Crop Snap");
    let mut fit_btn = Button::new(800, 10, 80, 30, "Re-center");

    // ---- preview + readout ----
    let mut frame = Frame::new(20, 50, PREVIEW_W as i32, PREVIEW_H as i32, "");
    frame.set_frame(FrameType::FlatBox);
    frame.set_color(Color::from_rgb(70, 70, 90));
    let mut info = Frame::new(10, 555, 710, 25, "");
    info.set_align(Align::Inside | Align::Left);

    // ---- sidebar: palette tools (x = 735) ----
    let mut side_title = Frame::new(735, 50, 155, 20, "Color Options");
    side_title.set_label_font(fltk::enums::Font::HelveticaBold);
    let mut merge_spinner = Spinner::new(790, 80, 100, 30, "Merge\nTol:");
    merge_spinner.set_minimum(0.0);
    merge_spinner.set_maximum(200.0);
    merge_spinner.set_step(5.0);
    merge_spinner.set_value(10.0);
    let mut clean_btn = Button::new(735, 115, 155, 30, "Merge Shades");
    let mut target_spinner = Spinner::new(790, 155, 100, 30, "Target\nColors:");
    target_spinner.set_minimum(2.0);
    target_spinner.set_maximum(256.0);
    target_spinner.set_step(1.0);
    target_spinner.set_value(16.0);
    let mut fitmerge_btn = Button::new(735, 195, 155, 30, "Merge to Target");
    let mut predict = Frame::new(735, 225, 155, 30, "");
    predict.set_align(Align::Inside | Align::Left);
    let mut hue_spinner = Spinner::new(790, 250, 60, 30, "Hue\nWeight:");
    hue_spinner.set_minimum(0.5);
    hue_spinner.set_maximum(16.0);
    hue_spinner.set_step(0.5);
    hue_spinner.set_value(2.0);
    let mut pal_choice = Choice::new(790, 290, 100, 30, "Palette:");
    let palettes: Rc<RefCell<Vec<Palette>>> = Rc::new(RefCell::new({
        let mut v = builtins();
        v.extend(load_saved());
        v
    }));
    for p in palettes.borrow().iter() {
        pal_choice.add_choice(&p.name);
    }
    pal_choice.set_value(0);
    let scroll: Rc<Cell<i32>> = Rc::new(Cell::new(0)); // first visible swatch row
    let tool: Rc<Cell<u8>> = Rc::new(Cell::new(0)); // 0 = none, 1 = brush, 2 = eraser
    let brush_color: Rc<Cell<[u8; 3]>> = Rc::new(Cell::new([255, 255, 255]));

    let mut map_btn = Button::new(735, 330, 155, 30, "Map to palette");
    Frame::new(735, 365, 155, 15, "Lospec slug:");
    let mut slug_input = Input::new(735, 385, 155, 28, "");
    slug_input.set_tooltip("Lospec slug, e.g. endesga-32");
    let mut fetch_btn = Button::new(735, 418, 155, 30, "Fetch from Lospec");
    let mut hex_btn = Button::new(735, 453, 50, 30, "Load");
    let mut folder_btn = Button::new(790, 453, 50, 30, "Folder");
    let mut reload_btn = Button::new(845, 453, 45, 30, "Sync");
    let mut stray_spinner = Spinner::new(790, 495, 100, 30, "Strays:");
    stray_spinner.set_minimum(0.0);
    stray_spinner.set_maximum(3.0);
    stray_spinner.set_step(1.0);
    stray_spinner.set_value(1.0);
    let mut protect_spinner = Spinner::new(790, 535, 100, 30, "Contrast\nGuard:");
    protect_spinner.set_minimum(0.0);
    protect_spinner.set_maximum(442.0);
    protect_spinner.set_step(10.0);
    protect_spinner.set_value(100.0);
    let mut smooth_btn = Button::new(735, 575, 155, 30, "Smooth Strays");
    // ---- dither column (x = 905) ----
    let mut dither_title = Frame::new(905, 50, 95, 20, "Dither");
    dither_title.set_label_font(fltk::enums::Font::HelveticaBold);
    let mut dither_choice = Choice::new(905, 100, 95, 30, "Mode:");
    dither_choice.set_align(Align::Top | Align::Left);
    dither_choice.add_choice("None");
    dither_choice.add_choice("Bayer 2x2");
    dither_choice.add_choice("Bayer 4x4");
    dither_choice.add_choice("Bayer 8x8");
    dither_choice.add_choice("Floyd-Steinberg");
    dither_choice.set_value(0);
    let mut dither_spinner = Spinner::new(905, 160, 95, 30, "Strength:");
    dither_spinner.set_align(Align::Top | Align::Left);
    dither_spinner.set_minimum(0.0);
    dither_spinner.set_maximum(100.0);
    dither_spinner.set_step(1.0);
    dither_spinner.set_value(20.0);
    let mut swatch_title = Frame::new(905, 195, 95, 15, "Palette");
    let mut color_box = Frame::new(905, 465, 95, 20, "");
    color_box.set_frame(FrameType::BorderBox);
    color_box.set_color(Color::from_rgb(255, 255, 255));
    let mut brush_btn = Button::new(895, 490, 55, 30, "Brush");
    let mut erase_btn = Button::new(950, 490, 55, 30, "Erase");

    // ---- bottom bar (y = 590) ----
    let mut grid_check = CheckButton::new(630, 550, 60, 30, "Show Grid");
    let mut tol_spinner = Spinner::new(45, 590, 60, 30, "Tol:");
    tol_spinner.set_minimum(0.0);
    tol_spinner.set_maximum(255.0);
    tol_spinner.set_step(5.0);
    tol_spinner.set_value(255.0);
    let mut bg_btn = Button::new(115, 590, 100, 30, "Remove BG");
    let mut spinner = Spinner::new(275, 590, 60, 30, "Block:");
    spinner.set_minimum(1.0);
    spinner.set_maximum(128.0);
    spinner.set_step(0.1);
    spinner.set_value(8.0);
    let mut detect_btn = Button::new(345, 590, 70, 30, "Detect");
    let mut snap_btn = Button::new(425, 590, 80, 30, "Snap!");
    let mut pick_btn = Button::new(520, 590, 60, 30, "Pick");
    scale_spinner.set_tooltip("Upscale factor for Save (nearest-neighbor)");

    swatch_title.set_label_font(fltk::enums::Font::HelveticaBold);
    // swatch strip: shows the selected palette's colors (10px squares, 14 per row)
    let swatch = Frame::new(905, 210, 95, 250, "");
    {
        let (palettes, pal_choice, scroll) = (palettes.clone(), pal_choice.clone(), scroll.clone());
        let mut swatch = swatch.clone();
        swatch.draw(move |f| {
            draw::draw_rect_fill(f.x(), f.y(), f.w(), f.h(), Color::from_rgb(70, 70, 90));
            if let Some(p) = palettes.borrow().get(pal_choice.value().max(0) as usize) {
                let (cell, grid_h) = (19, f.h() - 18);
                let per_row = (f.w() / cell).max(1);
                let total_rows = (p.colors.len() as i32 + per_row - 1) / per_row;
                let vis_rows = grid_h / cell;
                let max_scroll = (total_rows - vis_rows).max(0);
                scroll.set(scroll.get().clamp(0, max_scroll));
                let first = scroll.get() * per_row;
                for (i, c) in p.colors.iter().enumerate().skip(first as usize) {
                    let k = i as i32 - first;
                    let (cx, cy) = (f.x() + (k % per_row) * cell, f.y() + (k / per_row) * cell);
                    if cy + cell > f.y() + grid_h {
                        break;
                    }
                    draw::draw_rect_fill(cx, cy, cell, cell, Color::from_rgb(c[0], c[1], c[2]));
                }
                draw::set_draw_color(Color::White);
                draw::set_font(fltk::enums::Font::Helvetica, 12);
                draw::draw_text2(
                    &format!("{} colors", p.colors.len()),
                    f.x(),
                    f.y() + grid_h,
                    f.w(),
                    18,
                    Align::Center,
                );
            }
        });
    }
    // swatch editing: right-click removes a color, shift+right-click clears (custom only)
    {
        let (palettes, pal_choice, scroll) = (palettes.clone(), pal_choice.clone(), scroll.clone());
        let mut swatch = swatch.clone();
        swatch.set_tooltip(
            "Right-click a color to remove it. Shift+right-click clears the palette. (custom only)",
        );
        let brush_color = brush_color.clone();
        let mut color_box = color_box.clone();
        swatch.handle(move |f, ev| {
            if matches!(ev, Event::MouseWheel) {
                let d = match app::event_dy() {
                    app::MouseWheel::Down => 1,
                    app::MouseWheel::Up => -1,
                    _ => 0,
                };
                scroll.set((scroll.get() + d).max(0)); // draw clamps the top end
                f.redraw();
                return true;
            }
            if matches!(ev, Event::Push) && app::event_button() == 1 {
                let idx = pal_choice.value().max(0) as usize;
                if let Some(p) = palettes.borrow().get(idx) {
                    let (cell, per_row) = (19, (f.w() / 19).max(1));
                    let (cx, cy) = (
                        (app::event_x() - f.x()) / cell,
                        (app::event_y() - f.y()) / cell,
                    );
                    let i = ((cy + scroll.get()) * per_row + cx) as usize;
                    if cx < per_row && i < p.colors.len() {
                        let c = p.colors[i];
                        brush_color.set(c);
                        color_box.set_color(Color::from_rgb(c[0], c[1], c[2]));
                        color_box.redraw();
                    }
                }
                return true;
            }
            if !matches!(ev, Event::Push) || app::event_button() != 3 {
                return false;
            }
            let idx = pal_choice.value().max(0) as usize;
            let mut l = palettes.borrow_mut();
            let Some(p) = l.get_mut(idx) else {
                return true;
            };
            if p.name != "custom" {
                return true;
            }
            if app::is_event_shift() {
                p.colors.clear();
            } else {
                let (cell, per_row) = (19, (f.w() / 19).max(1));
                let (cx, cy) = (
                    (app::event_x() - f.x()) / cell,
                    (app::event_y() - f.y()) / cell,
                );
                let i = ((cy + scroll.get()) * per_row + cx) as usize;
                if cx < per_row && i < p.colors.len() {
                    p.colors.remove(i);
                }
            }
            save_palette(p);
            f.redraw();
            true
        });
    }

    let mut ver = Frame::new(905, 610, 100, 20, "");
    ver.set_label(&format!("v{VERSION}"));
    ver.set_align(Align::Inside | Align::Right);
    ver.set_label_size(11);
    ver.set_label_color(Color::from_rgb(140, 140, 160));

    win.end();

    //tooltips
    open_btn.set_tooltip("Open a PNG, JPG or WebP image");
    save_btn.set_tooltip("Save the snapped sprite as PNG (see Save x for upscaling)");
    undo_btn.set_tooltip("Undo the last change");
    redo_btn.set_tooltip("Redo");
    reset_btn.set_tooltip("Back to the image as first opened (undoable)");
    crop_btn.set_tooltip("Crop the canvas to the sprite (run Remove BG first)");
    dragcrop_btn.set_tooltip("Drag handles to crop. Wheel zooms, right-drag pans");
    apply_btn.set_tooltip("Commit the crop box (Ctrl+Enter)");
    snap_check.set_tooltip("Crop box edges jump in steps of Block size");
    grid_check.set_tooltip("Show where Snap will cut");
    fit_btn.set_tooltip("Reset zoom and pan");
    merge_spinner.set_tooltip("Colors closer than this get merged. Higher = fewer colors");
    clean_btn.set_tooltip("Merge near-duplicate colors");
    target_spinner.set_tooltip("Color count to aim for");
    fitmerge_btn.set_tooltip("Set Merge to reach the target color count");
    pal_choice.set_tooltip("Palette used by Map to palette");
    map_btn.set_tooltip("Snap every pixel to the nearest palette color");
    fetch_btn.set_tooltip("Download a palette from Lospec by slug");
    hex_btn.set_tooltip("Load a .hex palette file");
    stray_spinner.set_tooltip("Replace pixels with this many same-colored neighbors or fewer");
    protect_spinner.set_tooltip(
        "Pixels this far from their surroundings are kept (protects eyes, highlights)",
    );
    smooth_btn.set_tooltip("Remove stray pixels");
    tol_spinner.set_tooltip("How close to the background color counts as background");
    bg_btn.set_tooltip("Flood-fill the background from the edges");
    spinner.set_tooltip("Fake-pixel size in source pixels (decimals allowed)");
    detect_btn.set_tooltip("Estimate the fake-pixel size");
    snap_btn.set_tooltip("Convert the image to true pixels");
    hue_spinner.set_tooltip("Map to palette: higher keeps hues truer, lower keeps lightness truer");
    pick_btn.set_tooltip("Eyedropper: click the preview to add a color to the custom palette");
    folder_btn.set_tooltip("Open the folder where palettes are saved");
    reload_btn.set_tooltip("Re-scan the palettes folder and add any new .hex files");
    dither_choice.set_tooltip("Bayer = regular pattern, Floyd-Steinberg = scattered error diffusion. Applied when you click Map to palette");
    dither_spinner.set_tooltip("How strong the dither pattern is, in percent (0 = off)");
    brush_btn.set_tooltip("Brush (B): paint true pixels on the snapped sprite with the selected color (click a swatch to choose). Do touch-ups last: Remove BG and Crop redo the snap");
    erase_btn.set_tooltip("Erase (E): erase pixels to transparent on the snapped sprite");
    color_box.set_tooltip("Current brush color. Click a palette swatch to change it");

    // keyboard shortcuts (each one fires the button's own callback)
    open_btn.set_shortcut(Shortcut::Ctrl | 'o');
    save_btn.set_shortcut(Shortcut::Ctrl | 's');
    undo_btn.set_shortcut(Shortcut::Ctrl | 'z');
    redo_btn.set_shortcut(Shortcut::Ctrl | 'y');
    reset_btn.set_shortcut(Shortcut::Ctrl | 'r');
    snap_btn.set_shortcut(Shortcut::Ctrl | 'g'); // "go"
    bg_btn.set_shortcut(Shortcut::Ctrl | 'b');
    apply_btn.set_shortcut(Shortcut::Ctrl | Key::Enter);
    fit_btn.set_shortcut(Shortcut::Ctrl | 'f');
    brush_btn.set_shortcut(Shortcut::None | 'b');
    erase_btn.set_shortcut(Shortcut::None | 'e');

    win.show();

    //non-destructive workflow shit
    let source: Rc<RefCell<Option<RgbaImage>>> = Rc::new(RefCell::new(None));
    let original: Rc<RefCell<Option<RgbaImage>>> = Rc::new(RefCell::new(None));
    let history: Rc<RefCell<History>> = Rc::new(RefCell::new(History::default()));
    let result: Rc<RefCell<Option<RgbaImage>>> = Rc::new(RefCell::new(None));
    let crop: CropSlot = Rc::new(RefCell::new(Crop::default()));
    let recipe: Recipe = Rc::new(RefCell::new(Vec::new()));
    let pick: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let painted: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    {
        let (mut swatch, scroll) = (swatch.clone(), scroll.clone());
        pal_choice.set_callback(move |_| {
            scroll.set(0);
            swatch.redraw();
        });
    }

    {
        let pick = pick.clone();
        pick_btn.set_callback(move |b| {
            pick.set(!pick.get());
            b.set_label(if pick.get() { "Picking" } else { "Pick" });
        });
    }

    let view = View {
        frame: frame.clone(),
        info,
        grid: grid_check.clone(),
        block: spinner.clone(),
        source: source.clone(),
        result: result.clone(),
        crop: crop.clone(),
        snap_block: Rc::new(Cell::new(8.0)),
        cache: Rc::new(RefCell::new(None)),
        vp: Rc::new(Cell::new(FIT)),
        merge: merge_spinner.clone(),
        predict: predict.clone(),
        hover: Rc::new(Cell::new(None)),
        tool: tool.clone(),
    };

    //brush callback
    {
        let (tool, pick, mut erase_btn, mut pick_btn, mut view) = (
            tool.clone(),
            pick.clone(),
            erase_btn.clone(),
            pick_btn.clone(),
            view.clone(),
        );
        brush_btn.set_callback(move |b| {
            let on = tool.get() != 1;
            tool.set(if on { 1 } else { 0 });
            b.set_label(if on { "[Brush]" } else { "Brush" });
            erase_btn.set_label("Erase");
            pick.set(false);
            pick_btn.set_label("Pick");
            view.hover.set(None);
            view.refresh();
        });
    }

    //eraser callback
    {
        let (tool, pick, mut brush_btn, mut pick_btn, mut view) = (
            tool.clone(),
            pick.clone(),
            brush_btn.clone(),
            pick_btn.clone(),
            view.clone(),
        );
        erase_btn.set_callback(move |b| {
            let on = tool.get() != 2;
            tool.set(if on { 2 } else { 0 });
            b.set_label(if on { "[Erase]" } else { "Erase" });
            brush_btn.set_label("Brush");
            pick.set(false);
            pick_btn.set_label("Pick");
            view.hover.set(None);
            view.refresh();
        });
    }

    // Open
    {
        let (source, result, original, history, mut view, crop, recipe, painted) = (
            source.clone(), result.clone(), original.clone(), history.clone(),
            view.clone(), crop.clone(), recipe.clone(), painted.clone(),
        );
        open_btn.set_callback(move |_| {
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseFile);
            chooser.set_filter("Images\t*.{png,jpg,jpeg,webp}");
            chooser.show();
            let path = chooser.filename();
            if path.as_os_str().is_empty() {
                return;
            } // cancelled
            match image::open(&path) {
                Ok(img) => {
                    *history.borrow_mut() = History::default();
                    recipe.borrow_mut().clear();
                    let rgba = img.to_rgba8();
                    *original.borrow_mut() = Some(rgba.clone());
                    *source.borrow_mut() = Some(rgba);
                    *result.borrow_mut() = None;
                    crop.borrow_mut().active = false;
                    painted.set(false);
                    view.vp.set(FIT);
                    view.refresh();
                }
                Err(e) => dialog::alert_default(&format!("Couldn't open: {e}")),
            }
        });
    }

    // Snap
    {
        let (source, result, original, history, mut view, spinner, recipe, painted) = (
            source.clone(), result.clone(), original.clone(), history.clone(), view.clone(), spinner.clone(),
            recipe.clone(), painted.clone(),
        );
        snap_btn.set_callback(move |_| {
            let src = source.borrow().clone();
            if let Some(src) = src {
                let was_painted = painted.get();
                if !confirm_discard(&painted) {
                    return;
                }
                record(
                    &history,
                    snapshot(
                        &original,
                        &source,
                        &result,
                        &recipe,
                        view.snap_block.get(),
                             was_painted,
                    ),
                );
                view.snap_block.set(spinner.value() as f32);
                {
                    let mut r = recipe.borrow_mut();
                    r.clear();
                    r.push(Step::Clean(10.0)); // auto-clean after every snap
                }
                let out = reduce_colors(&downscale(&src, spinner.value() as f32), 10.0);
                *result.borrow_mut() = Some(out);
                view.refresh();
            }
        });
    }

    // Save: the snapped sprite if there is one, otherwise the working image
    {
        let (result, source, scale_spinner) =
            (result.clone(), source.clone(), scale_spinner.clone());
        save_btn.set_callback(move |_| {
            let img = result.borrow().clone().or_else(|| source.borrow().clone());
            if let Some(img) = img {
                let mut chooser =
                    dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseSaveFile);
                chooser.set_filter("*.png");
                chooser.set_option(dialog::NativeFileChooserOptions::SaveAsConfirm);
                chooser.show();
                let mut path = chooser.filename();
                if path.as_os_str().is_empty() {
                    return;
                } // cancelled
                if path.extension().is_none() {
                    path.set_extension("png");
                }
                let k = (scale_spinner.value() as u32).max(1);
                let out = if k == 1 {
                    img
                } else {
                    imageops::resize(
                        &img,
                        img.width() * k,
                        img.height() * k,
                        imageops::FilterType::Nearest,
                    )
                };
                if let Err(e) = out.save(&path) {
                    dialog::alert_default(&format!("Couldn't save: {e}"));
                }
            }
        });
    }

    // Remove BG
    {
        let (source, original, result, history, mut view, tol_spinner, spinner, recipe, palettes, painted) = (
            source.clone(), original.clone(), result.clone(), history.clone(), view.clone(),
            tol_spinner.clone(), spinner.clone(), recipe.clone(), palettes.clone(), painted.clone(),
        );
        bg_btn.set_callback(move |_| {
            let orig = original.borrow().clone();
            if let Some(orig) = orig {
                let was_painted = painted.get();
                if result.borrow().is_some() && !confirm_discard(&painted) {
                    return;
                }
                record(
                    &history,
                    snapshot(
                        &original,
                        &source,
                        &result,
                        &recipe,
                        view.snap_block.get(),
                             was_painted,
                    ),
                );
                view.snap_block.set(spinner.value() as f32);
                let had_snap = result.borrow().is_some();
                let cleaned = remove_bg(&orig, tol_spinner.value() as f32);
                let snapped = if had_snap {
                    Some(run_recipe(
                        downscale(&cleaned, spinner.value() as f32),
                                    &recipe.borrow(),
                                    &palettes.borrow(),
                    ))
                } else {
                    None
                };
                *source.borrow_mut() = Some(cleaned);
                *result.borrow_mut() = snapped;
                view.refresh();
            }
        });
    }

    // Undo
    {
        let (original, source, result, history, mut view, crop, recipe, mut spinner, painted) = (
            original.clone(), source.clone(), result.clone(), history.clone(), view.clone(),
                                                                                                 crop.clone(), recipe.clone(), spinner.clone(), painted.clone(),
        );
        undo_btn.set_callback(move |_| {
            let prev = history.borrow_mut().undo.pop();
            if let Some(prev) = prev {
                let cur = snapshot(
                    &original, &source, &result, &recipe, view.snap_block.get(), painted.get(),
                );
                history.borrow_mut().redo.push(cur);
                apply(
                    prev, &original, &source, &result, &recipe, &view.snap_block, &painted,
                );
                spinner.set_value(view.snap_block.get() as f64);
                crop.borrow_mut().active = false;
                view.refresh();
            }
        });
    }

    // Redo
    {
        let (original, source, result, history, mut view, crop, recipe, mut spinner, painted) = (
            original.clone(), source.clone(), result.clone(), history.clone(), view.clone(),
                                                                                                 crop.clone(), recipe.clone(), spinner.clone(), painted.clone(),
        );
        redo_btn.set_callback(move |_| {
            let next = history.borrow_mut().redo.pop();
            if let Some(next) = next {
                let cur = snapshot(
                    &original, &source, &result, &recipe, view.snap_block.get(), painted.get(),
                );
                history.borrow_mut().undo.push(cur);
                apply(
                    next, &original, &source, &result, &recipe, &view.snap_block, &painted,
                );
                spinner.set_value(view.snap_block.get() as f64);
                crop.borrow_mut().active = false;
                view.refresh();
            }
        });
    }

    // Reset: back to the freshly opened image, default spinners (undoable)
    {
        let (original, source, result, history, mut view, crop, recipe, painted) = (
            original.clone(), source.clone(), result.clone(), history.clone(), view.clone(),
            crop.clone(), recipe.clone(), painted.clone(),
        );
        reset_btn.set_callback(move |_| {
            let orig = original.borrow().clone();
            if let Some(orig) = orig {
                record(
                    &history,
                    snapshot(&original, &source, &result, &recipe, view.snap_block.get(), painted.get()),
                );
                apply(
                    State {
                        original: Some(orig.clone()),
                      source: Some(orig),
                      result: None,
                      recipe: Vec::new(),
                      block: view.snap_block.get(),
                      painted: false,
                    },
                    &original,
                    &source,
                    &result,
                    &recipe,
                    &view.snap_block,
                    &painted,
                );
                crop.borrow_mut().active = false;
                painted.set(false);
                view.vp.set(FIT);
                view.refresh();
            }
        });
    }

    // Grid toggle
    {
        let mut view = view.clone();
        grid_check.set_callback(move |_| view.refresh());
    }

    // Block spinner: redraw grid and readout live
    {
        let mut view = view.clone();
        spinner.set_callback(move |_| view.refresh());
    }

    // Crop to sprite
    {
        let (source, original, result, history, mut view, spinner, recipe, palettes, painted) = (
            source.clone(), original.clone(), result.clone(), history.clone(), view.clone(),
            spinner.clone(), recipe.clone(), palettes.clone(), painted.clone(),
        );
        crop_btn.set_callback(move |_| {
            let src = source.borrow().clone();
            let orig = original.borrow().clone();
            if let (Some(src), Some(orig)) = (src, orig) {
                if let Some((x, y, w, h)) = content_bounds(&src) {
                    let was_painted = painted.get();
                    if result.borrow().is_some() && !confirm_discard(&painted) {
                        return;
                    }
                    record(
                        &history,
                        snapshot(
                            &original,
                            &source,
                            &result,
                            &recipe,
                            view.snap_block.get(),
                                 was_painted,
                        ),
                    );
                    view.snap_block.set(spinner.value() as f32);
                    let cropped = imageops::crop_imm(&src, x, y, w, h).to_image();
                    let orig_cropped = imageops::crop_imm(&orig, x, y, w, h).to_image();
                    let snapped = if result.borrow().is_some() {
                        Some(run_recipe(
                            downscale(&cropped, spinner.value() as f32),
                                        &recipe.borrow(),
                                        &palettes.borrow(),
                        ))
                    } else {
                        None
                    };
                    *original.borrow_mut() = Some(orig_cropped);
                    *source.borrow_mut() = Some(cropped);
                    *result.borrow_mut() = snapped;
                    view.refresh();
                } else {
                    dialog::alert_default("Nothing to crop. Run Remove BG first.");
                }
            }
        });
    }

    // Drag crop: toggle crop mode
    {
        let (source, result, crop, mut view) =
            (source.clone(), result.clone(), crop.clone(), view.clone());
        dragcrop_btn.set_callback(move |_| {
            let on_result = result.borrow().is_some();
            let img = result.borrow().clone().or_else(|| source.borrow().clone());
            if let Some(img) = img {
                {
                    let mut c = crop.borrow_mut();
                    c.active = !c.active;
                    c.drag = None;
                    if c.active {
                        let (x, y, w, h) =
                            content_bounds(&img).unwrap_or((0, 0, img.width(), img.height()));
                        c.rect = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
                        c.anchor = if on_result {
                            (0.0, 0.0)
                        } else {
                            (x as f32, y as f32)
                        };
                    }
                }
                view.refresh();
            }
        });
    }

    // Apply crop
    {
        let (source, original, result, history, crop, mut view, recipe, palettes, painted) = (
            source.clone(), original.clone(), result.clone(), history.clone(), crop.clone(),
            view.clone(), recipe.clone(), palettes.clone(), painted.clone(),
        );
        apply_btn.set_callback(move |_| {
            if !crop.borrow().active {
                return;
            }
            let src = source.borrow().clone();
            let orig = original.borrow().clone();
            let snapped = result.borrow().is_some();
            if let (Some(src), Some(orig)) = (src, orig) {
                let (x0, y0, x1, y1) = crop.borrow().rect;
                // source-pixel region: real cut positions if snapped, plain pixels if not
                let (sx, sy, sw, sh) = if snapped {
                    let (xs, ys) = cuts(&src, view.snap_block.get());
                    let at =
                        |v: &Vec<u32>, p: f32| v[(p.round().max(0.0) as usize).min(v.len() - 1)];
                    let a = at(&xs, x0).min(src.width() - 1);
                    let b = at(&xs, x1).max(a + 1).min(src.width());
                    let c = at(&ys, y0).min(src.height() - 1);
                    let d = at(&ys, y1).max(c + 1).min(src.height());
                    (a, c, b - a, d - c)
                } else {
                    let sx = (x0.round().max(0.0) as u32).min(src.width() - 1);
                    let sy = (y0.round().max(0.0) as u32).min(src.height() - 1);
                    let sw = ((x1 - x0).round().max(1.0) as u32).min(src.width() - sx);
                    let sh = ((y1 - y0).round().max(1.0) as u32).min(src.height() - sy);
                    (sx, sy, sw, sh)
                };
                let was_painted = painted.get();
                if snapped && !confirm_discard(&painted) {
                    return;
                }
                record(
                    &history,
                    snapshot(
                        &original,
                        &source,
                        &result,
                        &recipe,
                        view.snap_block.get(),
                        was_painted,
                    ),
                );
                let cropped = imageops::crop_imm(&src, sx, sy, sw, sh).to_image();
                let orig_cropped = imageops::crop_imm(&orig, sx, sy, sw, sh).to_image();
                let new_result = if snapped {
                    Some(run_recipe(
                        downscale(&cropped, view.snap_block.get()),
                        &recipe.borrow(),
                        &palettes.borrow(),
                    ))
                } else {
                    None
                };
                *original.borrow_mut() = Some(orig_cropped);
                *source.borrow_mut() = Some(cropped);
                *result.borrow_mut() = new_result;
                crop.borrow_mut().active = false;
                view.refresh();
            }
        });
    }

    // Mouse handling on the preview
    {
        let (palettes, mut pal_choice, pick) = (palettes.clone(), pal_choice.clone(), pick.clone());
        let (tool, brush_color) = (tool.clone(), brush_color.clone());
        let (original, history, recipe) = (original.clone(), history.clone(), recipe.clone());
        let mut swatch = swatch.clone();
        let (source, result, crop, spinner, snap_check, mut view) = (
            source.clone(),
            result.clone(),
            crop.clone(),
            spinner.clone(),
            snap_check.clone(),
            view.clone(),
        );
        let painted = painted.clone();
        let mut pan_last: Option<(i32, i32)> = None;
        let mut painting = false;
        let mut last: Option<(i32, i32)> = None;
        frame.handle(move |f, ev| {
            let dims = match (result.borrow().as_ref(), source.borrow().as_ref()) {
                (Some(r), _) => Some((r.width(), r.height())),
                (None, Some(s)) => Some((s.width(), s.height())),
                _ => None,
            };
            let Some((iw_u, ih_u)) = dims else {
                return false;
            };
            let (mx, my) = (app::event_x() - f.x(), app::event_y() - f.y());

            // wheel zoom, anchored at the cursor
            if matches!(ev, Event::MouseWheel) {
                let v = view.vp.get();
                let (scale, ox, oy) = xform(iw_u, ih_u, v);
                let (fx, fy) = ((mx as f32 - ox) / scale, (my as f32 - oy) / scale);
                let z = match app::event_dy() {
                    app::MouseWheel::Up => v.z * 1.25,
                    app::MouseWheel::Down => v.z / 1.25,
                    _ => return true,
                }
                .clamp(1.0, 64.0);
                let scale2 = scale / v.z * z;
                let (cx, cy) = if z <= 1.0 {
                    (0.5, 0.5)
                } else {
                    (
                        ((PREVIEW_W as f32 / 2.0 - (mx as f32 - fx * scale2))
                            / scale2
                            / iw_u as f32)
                            .clamp(0.0, 1.0),
                        ((PREVIEW_H as f32 / 2.0 - (my as f32 - fy * scale2))
                            / scale2
                            / ih_u as f32)
                            .clamp(0.0, 1.0),
                    )
                };
                view.vp.set(Zoom { z, cx, cy });
                view.refresh();
                return true;
            }

            // right/middle drag pans
            let btn = app::event_button();
            match ev {
                Event::Push if btn == 2 || btn == 3 => {
                    pan_last = Some((mx, my));
                    return true;
                }
                Event::Drag if pan_last.is_some() => {
                    let (lx, ly) = pan_last.unwrap();
                    let v = view.vp.get();
                    let (scale, _, _) = xform(iw_u, ih_u, v);
                    view.vp.set(Zoom {
                        z: v.z,
                        cx: (v.cx - (mx - lx) as f32 / (scale * iw_u as f32)).clamp(0.0, 1.0),
                        cy: (v.cy - (my - ly) as f32 / (scale * ih_u as f32)).clamp(0.0, 1.0),
                    });
                    pan_last = Some((mx, my));
                    view.refresh();
                    return true;
                }
                Event::Released if pan_last.is_some() => {
                    pan_last = None;
                    return true;
                }
                _ => {}
            }

            // brush cursor tracking
            if tool.get() != 0 && result.borrow().is_some() {
                if matches!(ev, Event::Enter) {
                    return true;
                } // needed to receive Move events
                if matches!(ev, Event::Move | Event::Drag | Event::Push) {
                    let (scale, ox, oy) = xform(iw_u, ih_u, view.vp.get());
                    let (fx, fy) = ((mx as f32 - ox) / scale, (my as f32 - oy) / scale);
                    let h = if fx >= 0.0 && fy >= 0.0 && fx < iw_u as f32 && fy < ih_u as f32 {
                        Some((fx as i32, fy as i32))
                    } else {
                        None
                    };
                    if view.hover.get() != h {
                        view.hover.set(h);
                        view.refresh();
                    }
                }
                if matches!(ev, Event::Leave) {
                    view.hover.set(None);
                    view.refresh();
                }
            }

            // eyedropper: left click adds the pixel under the cursor to "custom"
            if pick.get() && matches!(ev, Event::Push) && btn == 1 {
                let (scale, ox, oy) = xform(iw_u, ih_u, view.vp.get());
                let (ix, iy) = ((mx as f32 - ox) / scale, (my as f32 - oy) / scale);
                if ix >= 0.0 && iy >= 0.0 && ix < iw_u as f32 && iy < ih_u as f32 {
                    let px = match (result.borrow().as_ref(), source.borrow().as_ref()) {
                        (Some(r), _) => *r.get_pixel(ix as u32, iy as u32),
                        (None, Some(s)) => *s.get_pixel(ix as u32, iy as u32),
                        _ => return false,
                    };
                    if px[3] > 0 {
                        add_to_custom(&palettes, &mut pal_choice, [px[0], px[1], px[2]]);
                        swatch.redraw();
                    }
                }
                return true;
            }
            // brush / eraser: paint true pixels on the snapped sprite
            if tool.get() != 0 && !crop.borrow().active && result.borrow().is_some() {
                let down = matches!(ev, Event::Push) && btn == 1;
                let drag = matches!(ev, Event::Drag) && painting;
                if matches!(ev, Event::Released) && painting {
                    painting = false;
                    last = None;
                    return true;
                }
                if down || drag {
                    if down {
                        // one undo step per stroke
                        record(
                            &history,
                            snapshot(&original, &source, &result, &recipe, view.snap_block.get(), painted.get()),
                        );
                        painting = true;
                        painted.set(true);
                        last = None;
                    }
                    let (scale, ox, oy) = xform(iw_u, ih_u, view.vp.get());
                    let (ix, iy) = ((mx as f32 - ox) / scale, (my as f32 - oy) / scale);
                    if ix >= 0.0 && iy >= 0.0 && ix < iw_u as f32 && iy < ih_u as f32 {
                        if let Some(img) = result.borrow_mut().as_mut() {
                            let c = brush_color.get();
                            let px = if tool.get() == 1 {
                                image::Rgba([c[0], c[1], c[2], 255])
                            } else {
                                image::Rgba([0, 0, 0, 0])
                            };
                            let cur = (ix as i32, iy as i32);
                            paint_line(img, last.unwrap_or(cur), cur, px);
                            last = Some(cur);
                        }
                        view.refresh();
                    }
                    return true;
                }
            }
            if !crop.borrow().active {
                return false;
            }
            if !matches!(ev, Event::Push | Event::Drag | Event::Released) {
                return false;
            }
            let on_result = result.borrow().is_some();
            let (iw, ih) = match (result.borrow().as_ref(), source.borrow().as_ref()) {
                (Some(r), _) => (r.width() as f32, r.height() as f32),
                (None, Some(s)) => (s.width() as f32, s.height() as f32),
                _ => return false,
            };
            let (scale, ox, oy) = xform(iw_u, ih_u, view.vp.get());
            let ix = (mx as f32 - ox) / scale;
            let iy = (my as f32 - oy) / scale;

            {
                let mut c = crop.borrow_mut();
                match ev {
                    Event::Push => {
                        let (x0, y0, x1, y1) = c.rect;
                        let tol = 14.0 / scale;
                        let (dl, dr) = ((ix - x0).abs(), (ix - x1).abs());
                        let (dt, db) = ((iy - y0).abs(), (iy - y1).abs());
                        let near_y = iy >= y0 - tol && iy <= y1 + tol;
                        let near_x = ix >= x0 - tol && ix <= x1 + tol;
                        let mut l = near_y && dl <= tol;
                        let mut r = near_y && dr <= tol;
                        let mut t = near_x && dt <= tol;
                        let mut b = near_x && db <= tol;
                        if l && r {
                            if dl <= dr { r = false } else { l = false }
                        }
                        if t && b {
                            if dt <= db { b = false } else { t = false }
                        }
                        let inside =
                            ix > x0 + tol && ix < x1 - tol && iy > y0 + tol && iy < y1 - tol;
                        c.drag = if l || r || t || b {
                            Some(Drag {
                                l,
                                r,
                                t,
                                b,
                                mv: false,
                            })
                        } else if inside {
                            Some(Drag {
                                mv: true,
                                ..Default::default()
                            })
                        } else {
                            None
                        };
                        c.start = (ix, iy);
                        c.start_rect = c.rect;
                        c.grab = (
                            if l {
                                ix - x0
                            } else if r {
                                ix - x1
                            } else {
                                0.0
                            },
                            if t {
                                iy - y0
                            } else if b {
                                iy - y1
                            } else {
                                0.0
                            },
                        );
                    }
                    Event::Drag => {
                        if let Some(d) = c.drag {
                            let step = if on_result {
                                1.0
                            } else if snap_check.is_checked() {
                                (spinner.value() as f32).max(1.0)
                            } else {
                                1.0
                            };
                            let (ax, ay) = c.anchor;
                            let (gx, gy) = c.grab;
                            let (ex, ey) = (ix - gx, iy - gy);
                            let snx = |v: f32| ax + ((v - ax) / step).round() * step;
                            let sny = |v: f32| ay + ((v - ay) / step).round() * step;
                            let (sx0, sy0, sx1, sy1) = c.start_rect;
                            let mut r = c.rect;
                            if d.mv {
                                let (w, h) = (sx1 - sx0, sy1 - sy0);
                                let nx = snx(sx0 + ix - c.start.0).max(0.0).min((iw - w).max(0.0));
                                let ny = sny(sy0 + iy - c.start.1).max(0.0).min((ih - h).max(0.0));
                                r = (nx, ny, nx + w, ny + h);
                            } else {
                                if d.l {
                                    r.0 = snx(ex).max(0.0).min(r.2 - step);
                                }
                                if d.r {
                                    r.2 = snx(ex).min(iw).max(r.0 + step);
                                }
                                if d.t {
                                    r.1 = sny(ey).max(0.0).min(r.3 - step);
                                }
                                if d.b {
                                    r.3 = sny(ey).min(ih).max(r.1 + step);
                                }
                            }
                            c.rect = r;
                        }
                    }
                    _ => c.drag = None,
                }
            }
            view.refresh();
            true
        });
    }

    // Fit view
    {
        let mut view = view.clone();
        fit_btn.set_callback(move |_| {
            view.vp.set(FIT);
            view.refresh();
        });
    }

    // Detect block size
    {
        let (source, mut view, mut spinner) = (source.clone(), view.clone(), spinner.clone());
        detect_btn.set_callback(move |_| {
            let src = source.borrow().clone();
            if let Some(src) = src {
                match detect_size(&src) {
                    Some(s) => {
                        spinner.set_value((s * 10.0).round() as f64 / 10.0);
                        view.refresh();
                    }
                    None => dialog::alert_default("Couldn't find a repeating pattern."),
                }
            }
        });
    }

    // Clean palette
    {
        let (original, source, result, history, mut view, merge_spinner, recipe, painted) = (
            original.clone(), source.clone(), result.clone(), history.clone(), view.clone(),
                                                                                             merge_spinner.clone(), recipe.clone(), painted.clone(),
        );
        clean_btn.set_callback(move |_| {
            let res = result.borrow().clone();
            match res {
                Some(res) => {
                    record(
                        &history,
                        snapshot(&original, &source, &result, &recipe, view.snap_block.get(), painted.get()),
                    );
                    *result.borrow_mut() = Some(reduce_colors(&res, merge_spinner.value() as f32));
                    recipe
                    .borrow_mut()
                    .push(Step::Clean(merge_spinner.value() as f32));
                    view.refresh();
                }
                None => dialog::alert_default("Snap first, then clean the sprite."),
            }
        });
    }

    // Merge spinner: refresh readout and predictor
    {
        let mut view = view.clone();
        merge_spinner.set_callback(move |_| view.refresh());
    }

    // Fit Merge to target color count
    {
        let (result, mut merge_spinner, target_spinner, mut view) = (
            result.clone(), merge_spinner.clone(), target_spinner.clone(), view.clone(),
        );
        fitmerge_btn.set_callback(move |_| {
            let res = result.borrow().clone();
            match res {
                Some(r) => {
                    let t = tol_for_target(&r, target_spinner.value() as usize)
                        .ceil()
                        .min(200.0);
                    merge_spinner.set_value(t as f64);
                    view.refresh();
                }
                None => dialog::alert_default("Snap first, then fit the palette."),
            }
        });
    }

    // Map to palette
    {
        let (original, source, result, history, mut view, pal_choice, palettes, recipe, painted) = (
            original.clone(), source.clone(), result.clone(), history.clone(), view.clone(),
                                                                                                    pal_choice.clone(), palettes.clone(), recipe.clone(), painted.clone(),
        );
        map_btn.set_callback(move |_| {
            let res = result.borrow().clone();
            match res {
                Some(res) => {
                    let pals = palettes.borrow();
                    if let Some(p) = pals.get(pal_choice.value().max(0) as usize) {
                        record(
                            &history,
                            snapshot(&original, &source, &result, &recipe, view.snap_block.get(), painted.get()),
                        );
                        let mode = dither_choice.value().max(0) as u8;
                        let dither = if mode > 0 {
                            dither_spinner.value() as f32 / 100.0
                        } else {
                            0.0
                        };
                        *result.borrow_mut() = Some(map_to_palette(
                            &res,
                            p,
                            hue_spinner.value() as f32,
                                                                   dither,
                                                                   mode,
                        ));
                        recipe.borrow_mut().push(Step::Map(
                            p.name.clone(),
                                                           hue_spinner.value() as f32,
                                                           dither,
                                                           mode,
                        ));
                        view.refresh();
                    }
                }
                None => dialog::alert_default("Snap first, then map to a palette."),
            }
        });
    }

    // Fetch palette from Lospec (appends)
    {
        let (palettes, mut pal_choice, slug_input, mut swatch) = (
            palettes.clone(), pal_choice.clone(), slug_input.clone(), swatch.clone(),
        );
        fetch_btn.set_callback(move |b| {
            b.set_label("Fetching...");
            b.deactivate();
            app::flush();
            let res = fetch_lospec(&slug_input.value());
            b.set_label("Fetch from Lospec");
            b.activate();
            match res {
                Ok(p) => {
                    add_palette(&palettes, &mut pal_choice, p);
                    swatch.redraw();
                }
                Err(e) => dialog::alert_default(&e),
            }
        });
    }

    // Load a .hex file (appends)
    {
        let (palettes, mut pal_choice, mut swatch) =
            (palettes.clone(), pal_choice.clone(), swatch.clone());
        hex_btn.set_callback(move |_| {
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseFile);
            chooser.set_filter("Palettes\t*.hex");
            chooser.show();
            let path = chooser.filename();
            if path.as_os_str().is_empty() {
                return;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let colors = parse_hex(&text);
                    if colors.is_empty() {
                        dialog::alert_default("No colors found in that file.");
                    } else {
                        let name = path
                            .file_stem()
                            .map(|s| clean_name(&s.to_string_lossy()))
                            .unwrap_or_else(|| "custom".into());
                        add_palette(&palettes, &mut pal_choice, Palette { name, colors });
                        swatch.redraw();
                    }
                }
                Err(e) => dialog::alert_default(&format!("Couldn't read: {e}")),
            }
        });
    }

    // Open the palettes folder
    folder_btn.set_callback(move |_| {
        if let Err(e) = open_palette_dir() {
            dialog::alert_default(&e);
        }
    });

    // Reload palettes folder (adds new files, keeps existing ones)
    {
        let (palettes, mut pal_choice, mut swatch) =
            (palettes.clone(), pal_choice.clone(), swatch.clone());
        reload_btn.set_callback(move |_| {
            let fresh: Vec<Palette> = {
                let l = palettes.borrow();
                load_saved()
                    .into_iter()
                    .filter(|p| !l.iter().any(|q| q.name == p.name))
                    .collect()
            };
            let n = fresh.len();
            for p in fresh {
                pal_choice.add_choice(&p.name.replace('/', "-"));
                palettes.borrow_mut().push(p);
            }
            swatch.redraw();
            dialog::message_default(&format!("Found {n} new palette(s)."));
        });
    }

    // Smooth stray pixels
    {
        let (original, source, result, history, mut view, stray_spinner, protect_spinner, recipe, painted) = (
            original.clone(), source.clone(), result.clone(), history.clone(), view.clone(),
                                                                                                              stray_spinner.clone(), protect_spinner.clone(), recipe.clone(), painted.clone(),
        );
        smooth_btn.set_callback(move |_| {
            let res = result.borrow().clone();
            match res {
                Some(res) => {
                    record(
                        &history,
                        snapshot(&original, &source, &result, &recipe, view.snap_block.get(), painted.get()),
                    );
                    *result.borrow_mut() = Some(smooth_strays(
                        &res,
                        stray_spinner.value() as usize,
                                                              protect_spinner.value() as f32,
                    ));
                    recipe.borrow_mut().push(Step::Smooth(
                        stray_spinner.value() as usize,
                                                          protect_spinner.value() as f32,
                    ));
                    view.refresh();
                }
                None => dialog::alert_default("Snap first, then smooth."),
            }
        });
    }

    app.run().unwrap();
}
