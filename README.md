# screenshit

A small cross-platform (Windows / Linux) screenshot & annotation tool written
in Rust with [egui](https://github.com/emilk/egui).

Take a screenshot (or open any image), then crop it, draw arrows, lines and
text on it, and save it or copy the result to the clipboard.

## Usage

```
screenshit             # capture the full screen, then open the editor
screenshit <image>     # open an existing image in the editor
```

### How capture works

- **Windows** — the app captures the primary monitor itself via the Windows
  API (`xcap` crate). No external tools needed.
- **Linux** — there is no single capture API that works across X11 and every
  Wayland compositor, so the app shells out to the first native tool it finds:
  `gnome-screenshot`, `spectacle`, `grim` (Wayland), or `maim` / `scrot` /
  `import` (X11). Install any one of them.

For **region selection on Linux**, bind `scripts/screenshot.sh` to a hotkey
instead — it uses the native tool's region picker (`grim`+`slurp`,
`gnome-screenshot -a`, `maim -s`, …) and then opens this app with the result.
On Windows (and everywhere else) you can simply capture the full screen and
use the crop tool.

## Editor

| Tool   | How |
|--------|-----|
| Crop   | Drag a rectangle, then **Enter** (or the *Apply crop* button). **Esc** cancels. |
| Arrow  | Drag from tail to tip. |
| Line   | Drag. |
| Draw   | Drag to draw freehand; the stroke is spline-smoothed. |
| Rect   | Drag a box. Hold **Shift** for a square. |
| Circle | Drag the bounding box. Hold **Shift** for a perfect circle. |
| Text   | Click to place, type, **Enter** to commit, **Esc** to cancel. |

Color, stroke width and font size are in the toolbar.

| Shortcut | Action |
|----------|--------|
| `Ctrl+Z` | Undo (also undoes crops) |
| `Ctrl+Shift+Z` / `Ctrl+Y` | Redo |
| `Ctrl+C` | Copy the annotated image to the clipboard |
| `Ctrl+S` | Save via file dialog (PNG / JPEG / BMP) |

Note for X11 users: the clipboard content is owned by the app, so paste the
image *before* closing the editor.

## Building

```
cargo build --release
```

The binary is `target/release/screenshit`.

### Linux build dependencies

egui/winit and the dialogs need the usual development packages, e.g. on
Debian/Ubuntu:

```
sudo apt install build-essential pkg-config libxcb1-dev libxkbcommon-dev \
    libgl1-mesa-dev libwayland-dev
```

At runtime you'll also want one screenshot tool (see above); for region
select on wlroots compositors: `grim` + `slurp`.
