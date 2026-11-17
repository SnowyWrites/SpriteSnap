# SpriteSnap Manual

SpriteSnap turns AI-generated "pixel art" (which only looks pixelated) into a true pixel-grid sprite. It never edits your original file. The version number is shown in the bottom-right corner of the window.

## Quick workflow (aka how you'll probably end up using the app 99% of the time)

1. **Open** an image.
2. **Remove BG** (bottom bar) to make the background transparent.
3. **Crop to sprite** to trim the empty canvas.
4. **Detect** to estimate the fake-pixel size, and check it with **Show Grid**.
5. **Snap!** to produce the true-size sprite.
6. Optional polish: **Map to palette**, **Merge Shades**, **Smooth Strays**, then brush touch-ups.
7. **Save** (set **Save x** above 1 for an upscaled preview).

## Top bar

| Control | What it does |
|---|---|
| Open | Load a PNG, JPG or WebP. Clears undo history. |
| Save | Save the snapped sprite as PNG. If you haven't snapped yet, it saves the working image. |
| Save x | Upscale factor for Save (nearest-neighbor). 1 = true size. |
| Undo / Redo | Step through your changes (20 steps). |
| Reset | Back to the image as first opened (undoable). |
| Crop to sprite | Trim the canvas to the non-transparent area. Run Remove BG first. |
| Drag crop | Toggle crop mode: drag the yellow handles to resize, drag inside to move. |
| Apply | Commit the crop box. |
| Crop Snap | Crop edges jump in steps of Block size (only on an unsnapped image). |
| Re-center | Reset zoom and pan. |

### Keyboard shortcuts

| Key | Action |
|---|---|
| Ctrl+O / S | Open / Save |
| Ctrl+Z / Y | Undo / Redo |
| Ctrl+R | Reset |
| Ctrl+G | Snap! |
| Ctrl+B | Remove BG |
| Ctrl+Enter | Apply crop |
| Ctrl+F | Re-center |
| B / E | Brush / Erase |

## Preview

- **Mouse wheel:** zoom toward the cursor.
- **Right or middle drag:** pan.
- **Left drag:** crop handles (in crop mode).
- **Show Grid** shows where Snap will cut (before snapping), or one line per pixel (after).
- The line under the preview shows sprite size, color count and block size. In crop mode it shows the crop size and position.
- After snapping, crop mode works on the snapped sprite in whole sprite pixels.

## Bottom bar (the pipeline)

| Control | What it does |
|---|---|
| Tol | How close to the background color counts as background (0-255). Higher removes more. At 255, light parts of a sprite can disappear. |
| Remove BG | Flood-fills the background from the image edges. Re-run it with a new Tol any time. |
| Block | Fake-pixel size in source pixels. Decimals allowed (e.g. 9.6). |
| Detect | Estimates Block automatically. Works best after Remove BG and Crop to sprite. |
| Snap! | Converts the image to true pixels. Cuts follow the real borders in the art, so wobbly AI grids still work. |

## Sidebar: palette tools

Use these after snapping.

| Control | What it does |
|---|---|
| Merge Tol + Merge Shades | Merges colors closer than Merge Tol. Low = subtle, high = chunky. |
| Target Colors + Merge to Target | Finds the Merge Tol value that reaches about Target Colors. Targets are approximate. |
| Predictor | Shows how many colors the current Merge Tol would leave. |
| Palette dropdown + Map to palette | Snaps every pixel to the nearest color in the chosen palette. |
| Slug box + Fetch from Lospec | Downloads a palette by its Lospec slug (e.g. `endesga-32`). Needs an internet connection. The window may pause for a few seconds while it downloads. |
| Load | Loads a palette from a `.hex` file (one hex color per line). |
| Folder | Opens the folder where palettes are saved. |
| Sync | Re-scans that folder and adds any new `.hex` files. |
| Strays + Contrast Guard + Smooth Strays | Removes stray pixels. See below. |

**Smooth Strays**
- **Strays:** a pixel is replaced if it has this many same-colored neighbors or fewer (0-3). 0 is safest; 1 is a good default.
- **Contrast Guard:** pixels whose color differs from their surroundings by more than this are kept. Lower values protect more details (eyes, highlights).
- Run it after Map or Merge Shades, since it compares exact colors.

## Dither

Pick a **Mode** and **Strength** before clicking **Map to palette**.

- **Bayer 2x2 / 4x4 / 8x8:** a regular pattern. Larger sizes give finer gradients.
- **Floyd-Steinberg:** scatters the error for a more organic look.
- **Strength 0** turns dithering off.
- **Hue Weight** controls how much color counts versus lightness when matching. Higher keeps hues truer, lower keeps lightness truer.

## Touch-ups

After snapping, you can paint directly on the sprite.

- **Brush (B):** paint with the selected color. Click a swatch in the palette strip to choose the color (it shows in the box above the buttons).
- **Erase (E):** erase pixels to transparent.
- **Pick:** click the preview to add that pixel's color to the `custom` palette.
- Each stroke is one Undo step. The mouse wheel scrolls the swatch strip. On the `custom` palette, right-click a swatch to remove it, or Shift+right-click to clear it.
- **Do touch-ups last.** Snap!, Remove BG, Crop to sprite and Apply redo the sprite from the source, so they erase brush work. SpriteSnap warns you first, and Undo brings it back.

## Things worth knowing

- **Palette work survives re-snaps.** Remove BG, Crop to sprite and Apply replay your Merge, Map and Smooth steps. **Snap!** restarts the list with one automatic Merge Tol 10 clean. **Open** and **Reset** clear it. **Undo** and **Redo** restore the steps as they were at that point.
- **Saved palettes:** fetched and loaded palettes are stored as `.hex` files and reappear next time. Folder: `~/.config/spritesnap/palettes/` on Linux and macOS, `%APPDATA%\spritesnap\palettes\` on Windows. Delete a file to remove a palette.
- **Original is never touched.** Save writes a new PNG wherever you choose.

## Troubleshooting

| Symptom | Fix |
|---|---|
| Halo left around the sprite | Raise Tol, then Remove BG again. |
| Sprite itself is getting eaten | Lower Tol. |
| Detect returns double the real size | Set Block to half by hand. |
| Grid lines slice through fake pixels | Block is off. Try Detect, or nudge by 0.1. |
| Details vanish after Smooth Strays | Undo, lower Strays or Contrast Guard. |
| Palette result looks blotchy | Try a larger palette (e.g. `endesga-32`). |
| "Snap first" popup | Palette tools need a snapped sprite. |
