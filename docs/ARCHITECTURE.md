# Architecture (as built)

The target architecture is in [`../afniru_draft.md`](../afniru_draft.md). This
file describes only what exists now, and grows with the code.

| Module | Role |
|---|---|
| `main.rs` | CLI (`clap`), loads preferences, launches eframe. Nothing else. |
| `app.rs` | `App`: the `eframe::App`. Owns the preferences, the `Session`, the `ViewArea` and the `ControllerUi`. One frame (`draw`): resolve theme, handle dropped files, draw the shell, the controller (collecting `session::Action`s, applied right away), the readout and the views; returns the shell's own `Action`. Saves the controller state through eframe storage. |
| `prefs.rs` | `~/.afniru` in `~/.afnirc` format. Created with documented defaults on first run, never overwritten. |
| `data/` | `Dataset`: a volume (read by `afni-io`, or synthetic) plus display metadata (grid, voxel size, labels, TR). `load.rs` reads files; `synthetic.rs` is the demo phantom. |
| `ui/theme.rs` | Dark/light color tokens over egui's stock visuals. Follows macOS unless overridden. The slice canvas color is separate (black or white). |
| `geom/` | `orient.rs`: `GridOrient` (which voxel axis is R/A/S, from the matrix), `Plane` and its screen conventions (radiological by default, anterior up in axial, superior up elsewhere). `coords.rs`: voxel ↔ world, and the `CoordOrient` display conventions RAI/LPI. |
| `render/slice.rs` | `PlaneMap`: the one place that maps pixels ↔ voxels for a plane. `extract`: one slice in screen orientation, with edge letters and pixel size. Pure functions over `&[f32]`. |
| `render/compose.rs` | `Window` (auto 2–98 %) and gray RGBA compositing. |
| `ui/view_state.rs` | `Layout`, `ViewOptions`, and `Cursor` (the crosshair voxel and the active card). |
| `ui/view_area.rs` | `ViewArea`: lays out the cards, owns the cursor and the shared window, caches the displayed sub-brick, handles keys, draws the readout strip. |
| `ui/view_card.rs` | `PlaneCard`: one plane's card (image, crosshair, letters, scale bar, slider). Click/drag moves the cursor. Caches its texture; rebuilds only when the slice, window or dataset changes. |
| `session/` | No egui. `Session`: the `DatasetStore` (datasets behind `Arc`, by `DatasetId`), the controllers (`ControllerState`: underlay, sub-brick, `Cursor`) and a `generation` that changes whenever the displayed data does (the cache key). `Action` + `Session::apply` is the only way tools change it; `apply` validates. |
| `tools/` | `ToolId` (all shelf tools), the `Tool` trait, `ToolContext` (what a card may read), and `tools::tool()` (the registry). One folder per tool; Datasets and Crosshair exist. See `ADDING_A_TOOL.md`. |
| `session/overlay.rs` | `OverlayLayer` (a `LayerId`, dataset, OLay/Thr sub-bricks, color scale, ±/+, range, threshold, opacity, visible, A, B) and `OverlayChange`; the controller keeps a stack of them (`overlays`, bottom first); p- and q-values of the threshold and threshold-by-p, all through `afni-core`. `Session::apply` validates `AddOverlay`, `RemoveOverlay`, `MoveOverlay` and `Layer(id, change)`. |
| `render/layers.rs` | Evaluates the whole overlay stack at a list of voxels (a slice or one crosshair voxel): color-map layers, threshold masks, and rule masks whose letters read sub-bricks, coordinates or other layers (memoized; cycles guarded). |
| `render/mask.rs` | A `3dcalc` expression (`afni_core::calc::Expr`) over columns of values → on/off. |
| `render/resample.rs` | `nearest`: an overlay sub-brick onto the underlay grid (NaN outside). `ViewArea` keeps the result per layer. |
| `render/overlay.rs` | Builds `afni_core::overlay::OverlaySpec` from a layer and evaluates a slice with `evaluate_rows`; the boxed outline. |
| `ui/widgets/pbar.rs` | The color bar with ticks and threshold marks. |
| `tools/overlay/` | The Define Overlay card. |
| `ui/controller/` | `ControllerUi` (workspaces, rail flag; persisted). `workspace.rs`: pure data for named card arrangements. `shelf.rs`: tiles. `card_frame.rs`: card chrome. `rail.rs`: collapsed rail and pop-over. |
| `processing/` | No egui. File-neutral types (`ProcessingRun`, `ProcessingStep`, `StepArtifact`, `HealthAssessment`, `HealthEvidence`, `Health`, `EvidenceSource`). Adapters: `script.rs` (parse `proc.<subj>`), `discover.rs` (find the run, resolve artifacts, read QC files, fingerprint). `health.rs`: the rules and the aggregation. `review.rs`: `out.ss_review` and warning files. `model.rs`: `ProcessingModel` (selection by step id, refresh, View options). See `PROCESSING_RAIL.md`. |
| `ui/processing_rail.rs` | The right-hand Processing rail: subway line, health pills, expansion, detail window, strip and drawer. Returns `RailEvent`s (View, Refresh). |
| `session/graph.rs` | The tool dependency graph (`parent`, `children`, `downstream`), read from `Tool::attaches_to`. |
| `tools/clusterize/` | Clusterize, hooked under Define Overlay per layer: `compute.rs` (inputs for `afni_core::volume_cluster`, matches `3dClusterize`), `engine.rs` (cache; reruns on selection changes, not while the mouse is down), the card. |
| `ui/controller/hooks.rs`, `ui/widgets/chips.rs` | The spine/socket and stacked edge of hooked cards; the attach chips in a card's footer. |
| `session/` controllers | `Session::controllers` holds A, B, …; `Action::CloneController`, `SelectController`, `RemoveController`, `SetLinks`; `sync_crosshair` (linked crosshair through world coordinates) and `differences`. Each controller has its own `generation`; `App` keeps a `ViewArea` and a cluster `Engine` per controller. |
| `render/graph_image.rs`, `render/text.rs` | The Graph and smooth text (slice numbers, letters) as pixels for saved images. |
| `ui/fonts.rs` | Installs the Phosphor icon font once per context. |
| `ui/shell.rs` | Menu bar, toolbar, status bar. Draws and returns an `Action`; never mutates the session. |

Data flow of the Processing rail: `discover::locate` → `script::parse` → artifacts resolved on disk → `health::assess` → `ProcessingModel` (held by `App`, not by `Session`) → `ProcessingRail` draws it and returns `RailEvent`s → `App` asks, then opens the chosen dataset through the normal `open` path (so the crosshair keeps its world position).

Placeholders (a `//!` doc only, naming the milestone that fills them):
`analysis/` (the algorithms are in `afni-core`), the tool folders other
than Datasets, Overlay, Clusterize and Crosshair, most of `ui/widgets/`,
`ui/graph_view.rs`.

## What comes from afni-core

Statistics, colors, thresholds, compositing and the `3dcalc` expression evaluator (`calc`) are `afni-core`, not copied:
`stats::p_value` / `critical_value` (p and threshold-by-p), `fdr::q_value_for_threshold`,
`afni_colors::AfniColorScale`, `overlay::evaluate_rows` + `OverlaySpec`
(`Threshold`, `FadeModel`), `composite::composite_layers`, and `stat::StatSpec`
(which `afni-io` also re-exports). afniru depends on `afni-core` from the same
git source as `afni-io`, so the types are one and the same; the git-ignored
`.cargo/config.toml` patches both to the local checkout.

For dataset calculations, use the intent-oriented processing vocabulary from
`afni-core`: `combine_columns` combines selected sub-bricks into one derived
column; `Dataset::summarize_time_series` computes standard temporal statistics;
`summarize_time_series_with` handles a custom one- or multi-output temporal
calculation; and `transform_time_series` returns a same-length transformed
dataset. `SummaryOutput` describes each custom result. Avoid reintroducing the
older implementation-shaped `map_*`, `*_rows_*`, or `*_to_f64` names.

The existing Graph arithmetic operates on one already-extracted series for
interactive display, so it directly uses `afni_core::signal::Detrend` rather
than building a whole core dataset. Future whole-volume tools should use the
dataset operations above.

## Where this differs from the plan

- `data/` has one `Dataset` (summary + voxels) instead of the plan's own
  `Volume`/`SubBrick`; `afni-io` types are still only touched in `data/load.rs`.
- `geom/orient.rs` holds the grid/plane orientation; the RAI/LPI conventions
  and voxel ↔ world live in `geom/coords.rs`.
- The views edit `Cursor` directly while the crosshair is dragged or keys are
  pressed (high-frequency); tools go through `Action`s.

## Testing

- Pure logic (`geom`, `render`, `data`, `prefs`) has plain unit tests, some
  checked against AFNI's own output (`tests/fixtures/`).
- UI is tested with `egui_kittest`: `App::draw` takes a bare `Ui`, so snapshots
  render the whole shell, and tests can click widgets by label and then read
  `harness.state()`. Snapshots are in `tests/snapshots/`; regenerate with
  `UPDATE_SNAPSHOTS=1 cargo test` and look at the images before committing.

## Rules in force

- UI code returns actions; `app.rs` applies them.
- The crosshair is a voxel index (`Cursor::ijk`); world coordinates are derived from it on demand, never stored.
- Voxel order is `i + nx * (j + ny * k)`, as in `afni-io`.
- `unsafe` is forbidden; every file has a `//!` doc and public items have `///`.
- Preference names AFNI already defines keep their AFNI spelling and meaning;
  new ones start with `AFNIRU_`.
