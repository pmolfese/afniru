# Architecture (as built)

The target architecture is in [`../afniru_draft.md`](../afniru_draft.md). This
file describes only what exists now, and grows with the code.

| Module | Role |
|---|---|
| `main.rs` | CLI (`clap`), loads preferences, launches eframe. Nothing else. |
| `app.rs` | `App`: the `eframe::App`. Owns the preferences and opened datasets; runs one frame: resolve theme, handle dropped files, draw the shell, apply the returned `Action`. |
| `prefs.rs` | `~/.afniru` in `~/.afnirc` format. Created with documented defaults on first run, never overwritten. |
| `data/` | `Dataset`: a volume (read by `afni-io`, or synthetic) plus its summary (grid, voxel size, labels, TR). `load.rs` reads files; `synthetic.rs` is the demo phantom. |
| `ui/theme.rs` | Dark/light color tokens over egui's stock visuals. Follows macOS unless overridden. The slice canvas color is separate (black or white). |
| `geom/` | `GridOrient`: which voxel axis is R/A/S (from the voxel-to-RAS matrix). `Plane` and its screen conventions (radiological by default, anterior up in axial, superior up elsewhere). |
| `render/slice.rs` | `extract`: one slice, in screen orientation, with edge letters and pixel size. Pure functions over `&[f32]`. |
| `render/compose.rs` | `Window` (auto 2–98 %) and gray RGBA compositing. |
| `ui/view_card.rs` | One slice view. Caches its texture; rebuilds only when the slice, window or dataset changes. |
| `ui/shell.rs` | Menu bar, toolbar, status bar. Draws and returns an `Action`; never mutates the session. |

## Rules in force

- UI code returns actions; `app.rs` applies them.
- Voxel order is `i + nx * (j + ny * k)`, as in `afni-io`.
- `unsafe` is forbidden; every file has a `//!` doc and public items have `///`.
- Preference names AFNI already defines keep their AFNI spelling and meaning;
  new ones start with `AFNIRU_`.
