# afniru roadmap

Goal: a native, cross-platform Rust rebuild of the AFNI volume viewer that pays
homage to AFNI, feels modern, and is easy to extend. The design and the rules
the code follows are in [`afniru_draft.md`](afniru_draft.md). This file tracks
progress.

Status: ✅ done · 🚧 in progress · ⬜ not started

| # | Milestone | Status |
|---|-----------|--------|
| D | Design: mockups, plan, roadmap | ✅ |
| X | Cross-repo: afni-io p-values (stat ↔ p) | ⬜ |
| 0 | Skeleton: crate, window shell, loading, docs scaffold | 🚧 |
| 1 | One slice: extraction, window/level, view card | ✅ |
| 2 | Three linked views, layouts, coordinates | ⬜ |
| 3 | Controller shell: tool shelf, cards, rail, workspaces | ⬜ |
| 3A | `afni_proc.py` processing rail and dataset provenance | ⬜ |
| 4 | Overlay and threshold (one layer) | ⬜ |
| 5 | Multiple overlay layers | ⬜ |
| 6 | Hooked cards + Clusterize | ⬜ |
| 7 | Graph view + sub-brick selection | ⬜ |
| 8 | Controllers A/B, Lock, **Clone to compare** | ⬜ |
| 9 | InstaCorr | ⬜ |
| 10 | Pop-out windows (controller, cards) | ⬜ |
| — | Later (unordered) | ⬜ |

**Definition of done for every milestone**, in addition to its own checklist:
- [ ] `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test` pass
- [ ] Every new file has a `//!` module doc; every public item has a `///` doc (no `missing_docs` warnings)
- [ ] No file over ~800 lines without a note explaining why (hard cap 1,500)
- [ ] `docs/ARCHITECTURE.md` lists any new files; `docs/GLOSSARY.md` has any new AFNI terms
- [ ] AFNI behaviors cite their C source in doc comments
- [ ] A short entry in the Log at the bottom of this file
- [ ] The user gets a summary of what changed and the git commands to run (no commits by Claude)

---

## Milestone D — Design ✅

- [x] UI mockups rendered with real egui (`mockups/`, generator in `mockups/ui_mockup/`)
- [x] Layouts: 1×3 / 3×1 / 2×2 with the switcher in the upper right; 2×2 with the Graph in the 4th cell
- [x] Window decision: one window, controller docked, with rail and pop-out
- [x] Controller v2: tool shelf of tiles + collapsible/closable/reorderable cards, workspaces
- [x] Hooked cards: attach, spine/socket with data-flow labels, group folding
- [x] Compact right-side processing-rail concept: ordered `afni_proc.py`
  stages, separate selection and health state, expandable evidence, and links
  to the datasets produced by each stage (`mockups/afniru_15_processing_rail_right_compact.png`)
- [x] Direction for multiple overlay layers and Clone-to-compare (in the plan; mockups pending, see M5/M8)
- [x] `afniru_draft.md` rewritten as the design plan; this roadmap created
- [ ] Mockups still to do: multiple overlay layers (layer list + two overlay cards), Clone/compare layout with difference markers

## Milestone X — Cross-repo: p-values in afni-io ⬜

Lives in `../afni-io` (its own rules apply; the user commits there too).
Needed by M4. See "afni-io status" in the plan.

- [ ] Add to afni-io's roadmap as a new phase (this reverses its earlier "p-values go to afni-core" decision)
- [ ] `StatSpec::p_value(stat)` and `StatSpec::stat_for_p(p)`, matching AFNI's sidedness
- [ ] Port from `afni/src/thd_statpval.c` + `cdflib/`. Priority kinds: correlation, t, F, z, chi², then beta, binomial, gamma, Poisson, then the rest of the 23
- [ ] Seed the tests with sumaru's `stats.rs` reference values; add values from AFNI's `cdf -t2p` / `ccalc`
- [ ] FDR q lookup through `ThresholdCurve`
- [ ] Optional: partial reads of NIfTI volumes (`read_any_volumes` for NIfTI)

## Milestone 0 — Skeleton 🚧

- [x] `cargo new afniru` (edition 2024); eframe (wgpu backend) 0.36.2 + egui 0.36.2, matching sumaru's versions (sumaru: egui 0.36.2 / wgpu 30); `[lints.rust] unsafe_code = "forbid"`; `#![warn(missing_docs)]`
- [x] `afni-io = { path = "../afni-io" }` (git-ignored `.cargo/config.toml` patches afni-io's afni-core git dependency to `../afni-core`)
- [ ] Module skeleton from the plan's Architecture section. Empty modules get a `//!` doc explaining what will go there
- [x] `ui/theme.rs`: dark + light color tokens (from `mockups/ui_mockup/src/ui.rs`); follows the macOS appearance by default
- [x] `prefs.rs`: `~/.afniru` parser. Keys so far: `AFNIRU_THEME = System|Dark|Light`, `AFNIRU_CANVAS_BACKGROUND = Black|White`, `AFNI_LEFT_IS_LEFT`, `AFNI_SESSTRAIL`
- [x] `~/.afniru` is created with documented defaults on first run (`create_new`, never overwritten), in `~/.afnirc` format (`***ENVIRONMENT`, `//` comments). AFNI names keep their spelling: `AFNI_LEFT_IS_LEFT`, `AFNI_SESSTRAIL`; afniru-only keys are `AFNIRU_*`
- [ ] Preferences panel / write-back; more keys from `~/.afnirc` as the features that use them land (graph size, pbar, resample modes, …)
- [x] `ui/shell.rs`: toolbar (breadcrumb, layout switcher placeholder), status bar, in-window menu bar (File → Open, Quit)
- [x] Open datasets: `rfd` dialog, drag and drop onto the window, CLI paths (`clap`)
- [x] `data/load.rs`: `afni_io::volume::read_any` → `Dataset` (dims, voxel size, matrix, labels, TR). Stats (`StatSpec`) are not read yet; they arrive with M4
- [x] `data/synthetic.rs`: head phantom + fake t-map (from the mockup's `phantom.rs`); `--demo` opens the phantom
- [x] Status bar shows dataset name, dims, voxel size, sub-brick count (and TR)
- [x] `docs/ARCHITECTURE.md`, `docs/GLOSSARY.md`, `docs/ADDING_A_TOOL.md` (stub, filled in M3)
- [x] `README.md` for the repo: what afniru is, how to build and run

## Milestone 1 — One slice ✅

- [x] `render/slice.rs`: axial/coronal/sagittal extraction on the underlay grid, in screen orientation (oblique grids use the nearest cardinal axes; no resampling). `geom/orient.rs` maps voxel axes to R/A/S
- [x] `render/compose.rs` (grayscale only): window/level, with auto 2–98% percentiles (min–max fallback when they coincide, e.g. mostly-zero data)
- [x] `ui/view_card.rs`: header (plane picker + mm coordinate with letter), image with correct aspect from voxel sizes, zoom to fit, orientation letters, scale bar, slice slider with `index / max`, window controls with Auto. Nearest-neighbor texture (true voxels); the texture is rebuilt only when the slice, window or dataset changes
- [x] Page Up/Down change the slice
- [x] Tests: slice indexing for all three planes on a synthetic volume with known values (RAI, permuted and oblique grids, both display conventions); loaded voxel order matches `3dmaskdump` on `tests/fixtures/tiny2+orig`; `egui_kittest` snapshots of the card (`tests/snapshots/`)

Notes: the card's plane picker is a stopgap that M2 replaces with the three linked views. Display is on the dataset's own grid; the plane colors are already in `ui/theme.rs`.

## Milestone 2 — Three linked views, layouts, coordinates ⬜

- [ ] `ui/view_area.rs`: 1×3 / 3×1 / 2×2 (Graph placeholder in the 4th cell); switcher in the toolbar
- [ ] Crosshair: click to set it in all views, arrow keys move it, the plane colors (blue/green/orange) on sliders and crosshair lines, gap at the focus point
- [ ] `geom/`: ijk ↔ xyz (RAI), LPI display option, radiological/neurological toggle (default radiological)
- [ ] Readout: ijk, xyz with R/L/A/P/S/I letters, value under the crosshair
- [ ] Tests: ijk↔xyz round-trips; xyz matches `3dinfo -aform_real` and `3dmaskdump -xyz` on real data, including an oblique dataset

## Milestone 3 — Controller shell ⬜

- [ ] `tools/mod.rs`: `Tool` trait, `ToolId`, registry
- [ ] `ui/controller/`: tabs row (A only for now), tool shelf (tile states: open/collapsed/off), card stack
- [ ] Card chrome: drag handle (reorder), chevron (collapse to summary), ⬈ placeholder, × (close; state kept), pinned Datasets card
- [ ] First tools: Datasets (ULay picker), Crosshair (readout)
- [ ] Rail mode with pop-over cards
- [ ] Workspaces: named tool sets + order + fold state; save/restore (e.g. `eframe` storage)
- [ ] `docs/ADDING_A_TOOL.md` written, using Crosshair as the worked example
- [ ] UI snapshot tests (`egui_kittest`) for shelf states, collapsed card, rail

## Milestone 3A — `afni_proc.py` processing rail and dataset provenance ⬜

This is a compact, independent inspector on the **right** side of the view
area. It must not replace or widen the normal tool controller on the left. The
rail explains how the currently viewed data moved through an `afni_proc.py`
pipeline and makes intermediate outputs easy to inspect. Parsing processing
provenance and assessing health are afniru application services, not
`afni-core` algorithms.

### Discovery and processing model

- [ ] When afniru starts in an `afni_proc.py` results directory, detect the
  processing script and its generated inputs, outputs, review files, and QC
  artifacts automatically. Also allow the user to open a results directory
  explicitly; do not recursively search unrelated parent trees.
- [ ] Parse the executed processing order rather than assuming one fixed
  pipeline. Recognize common stages such as inputs, slice timing, volume
  registration, anatomical alignment, template warp (MNI/TLRC), spatial
  smoothing, percent-signal scaling, and regression, while preserving optional,
  omitted, repeated, or custom blocks.
- [ ] Define file-neutral application types such as `ProcessingRun`,
  `ProcessingStep`, `StepArtifact`, `HealthAssessment`, and `HealthEvidence`.
  Keep script/QC parsing in a provenance adapter so the UI does not depend on
  filenames or shell syntax.
- [ ] Associate every step with zero, one, or several artifacts, including
  anatomical and EPI datasets, masks, transforms, motion/censor summaries,
  regressors, and reports. Record each artifact's role, space, path, existence,
  and whether `afni-io` can open it.
- [ ] Preserve the source of every inferred fact (script option, generated
  command, dataset header, review value, or QC artifact) so the interface can
  explain its conclusions instead of presenting unexplained scores.

### Compact right-side rail

- [ ] Add a narrow, collapsible right rail headed `Processing`, with the run
  name in a small subtitle. It should use roughly 14–16% of a normal desktop
  window and collapse to a slim icon strip so the image views retain priority.
- [ ] Draw the actual steps from top to bottom as a subway-style line. Use
  neutral gray nodes for unselected steps and a blue node plus focus ring for
  the selected step. Selection color and health color are deliberately
  independent.
- [ ] Put a compact health pill/icon at the far right of each step: green for
  good, amber for caution, red for failed/danger, and neutral gray for unknown
  or not assessed. Never use color as the only signal; include an icon,
  accessible label/tooltip, and screen-reader text.
- [ ] Keep the default row to one line. Expanding the selected step shows only
  a concise reason and artifact count (for example `edge mismatch 2.4 mm` and
  `2 datasets · View`); full evidence and file lists open in a pop-over or
  detail card instead of permanently consuming view width.
- [ ] Make the rail responsive: at narrow widths it collapses automatically or
  becomes a temporary drawer. It must not squeeze the left controller or make
  the three slice views unusably small.
- [ ] Add keyboard navigation, focus styling, tooltips, and a legend for node
  selection versus health status.

### Health and dataset interaction

- [ ] Base health on inspectable evidence rather than filename heuristics
  alone. Initial checks may include missing/unreadable outputs, inconsistent
  grids or spaces, motion/censor and outlier summaries, alignment/warp overlap
  evidence, regression warnings, and non-finite or empty datasets when those
  measurements are available.
- [ ] Define the thresholds and provenance for every green/amber/red rule.
  Missing evidence yields `Unknown`, not a guessed `Good`; the detail view says
  exactly which check produced the state and why.
- [ ] Aggregate step health deterministically from individual checks while
  retaining every check result. A red check must not be hidden by several
  green checks.
- [ ] Clicking a step selects it and opens its concise health explanation.
  `View` presents the associated dataset or dataset pair and asks before
  replacing the current underlay/overlay selection. Preserve the crosshair in
  world coordinates when the new grid permits it.
- [ ] Support steps with multiple useful comparisons, especially pre/post
  alignment, pre/post warp, pre/post smoothing, and model input versus residual
  output. Reuse Controllers A/B later for side-by-side comparisons rather than
  inventing a separate comparison system here.
- [ ] Refresh the run model when outputs appear or change, without discarding
  the user's selected step or silently switching the displayed dataset.

### Verification

- [ ] Add small committed fixtures for a typical complete pipeline, omitted
  blocks, custom order, multiple runs, missing outputs, malformed scripts, and
  partial/in-progress results.
- [ ] Unit-test processing-order recovery, artifact association, health
  aggregation, and selection-to-dataset actions independently of egui.
- [ ] Add `egui_kittest` snapshots for the compact rail, selected/expanded
  step, all four health states, collapsed rail, narrow-window drawer, and long
  or custom step labels.
- [ ] Document supported `afni_proc.py`/QC inputs and every health rule in
  `docs/PROCESSING_RAIL.md`; add the new modules and data flow to
  `docs/ARCHITECTURE.md`.

## Milestone 4 — Overlay and threshold (one layer) ⬜

Needs Milestone X for p-values.

- [ ] `session/overlay.rs`: `OverlayLayer` (dataset, OLay/Thr sub-bricks, colormap, threshold, sign mode, range, opacity, visible)
- [ ] `render/resample.rs`: NN resampling of the overlay onto the underlay grid
- [ ] `analysis/color.rs` + `threshold.rs` (copied from sumaru, with source headers)
- [ ] Define Overlay card: pbar with threshold slider beside it, colormap, ± / +, range + auto, A/B toggles, `p` and `q` under the threshold
- [ ] Tests: compose gives the expected RGBA for known inputs; threshold `p` matches `cdf -t2p`

## Milestone 5 — Multiple overlay layers ⬜

- [ ] Mockup first: layer list in Datasets + two overlay cards
- [ ] `ControllerState.overlays: Vec<OverlayLayer>`; "+ Add overlay"; one Define Overlay card per layer
- [ ] Layer list: eye toggle, drag to reorder (top = drawn on top), opacity
- [ ] `render/compose.rs`: bottom-to-top alpha compositing of visible layers; optional outlines last
- [ ] Crosshair readout lists every layer's value with its color swatch
- [ ] Tests: two-layer composite order and opacity; hidden layers skipped

## Milestone 6 — Hooked cards + Clusterize ⬜

- [ ] `session/graph.rs`: tool dependency graph (attaches_to), recompute order downstream of a change
- [ ] `ui/controller/hooks.rs`: spine + ⛓ socket with data-flow label, attach chips (solid/dashed), drop slot while dragging, group fold via the socket
- [ ] `tools/clusterize/`: NN1/2/3 grid flood fill, min size (voxels or µL), bisided, per overlay layer; overlay restricted to surviving clusters
- [ ] Cluster table: size, peak, center of mass; clicking a row jumps the crosshair
- [ ] Golden tests: sizes and peaks match `3dClusterize` on a committed small stat dataset

## Milestone 7 — Graph view + sub-bricks ⬜

- [ ] Sub-brick selectors by index and label (ULay, and OLay/Thr per layer)
- [ ] `ui/graph_view.rs` in the 2×2 4th cell: time series at the crosshair, optional fit, stimulus blocks, current-TR marker, stats footer
- [ ] Graph card: matrix (1/3/5), ignore, detrend
- [ ] Tests: time series extraction at a voxel matches `3dmaskdump`

## Milestone 8 — Controllers A/B, Lock, Clone to compare ⬜

- [ ] Mockup first: compare layout (A above B, each 1×3) with difference markers
- [ ] `session/store.rs`: `Arc<Volume>` dataset store so controllers share voxel data
- [ ] Multiple controllers as tabs; each has its own state
- [ ] Lock / Link menu: crosshair, slices, zoom/pan (toggle each)
- [ ] **Clone A → B**: copies the full `ControllerState`; view area switches to the compare layout
- [ ] Differences: "≠" markers on changed settings in B's cards (A's value on hover); toolbar chip listing differences; "Sync from A" / "Push to B"
- [ ] Tests: clone gives identical state; diff lists exactly the changed fields; linked crosshair moves both

## Milestone 9 — InstaCorr ⬜

- [ ] Setup: dataset, ignore, blur, automask, despike, bandpass, seed radius (check `afni_instacorr.c`, `thd_incorrelate.c`)
- [ ] Live r-map as an overlay layer hooked under InstaCorr ("feeds r-map"); Shift-drag scrubs the seed
- [ ] Seed ring drawn on views (`Tool::paint_view`); Graph shows seed mean vs. voxel with r
- [ ] Workspace suggestion: turning InstaCorr on offers "Resting state"
- [ ] Tests: correlation at known voxels matches AFNI's InstaCorr output on a small fixture

## Milestone 10 — Pop-out windows ⬜

- [ ] Controller pops out into its own egui viewport and docks back
- [ ] Single cards pop out (e.g. Clusterize on a second monitor)
- [ ] Window positions remembered per workspace

## Later (unordered) ⬜

- [ ] Atlas / Whereami card (and "Atlas labels" attached to overlays/clusters)
- [ ] Draw ROI (brush cursor on views; save with `Brik::write`)
- [ ] Montage
- [ ] Save PNG / image sequences; `-com`-style scripting (`Action` from text)
- [ ] NIML link with sumaru and classic AFNI (crosshair sync); reuse sumaru's `afni.rs` and capture fixtures
- [ ] Compare extras: flicker, swipe divider, B − A difference overlay
- [ ] Icon font (`egui-phosphor`) replacing built-in glyphs
- [ ] GPU slice compositing if profiling calls for it
- [ ] Plugins (in-process `Tool` implementations first; WASM or subprocess + NIML later)
- [ ] Extract `afni-core` shared with sumaru

---

## Log

- 2026-10-03 — Added Milestone 3A for an `afni_proc.py`-aware processing and
  provenance rail. The compact rail stays on the right so the existing tool
  controller remains unchanged; gray/blue nodes represent selection while
  separate green/amber/red/gray pills represent evidence-backed health. A step
  can explain its assessment and offer its associated dataset(s) without
  silently changing the current view.

- 2026-10-02 — Design milestone. Mockups 01–13 rendered with egui 0.34.3 (since bumped to 0.36.2) via
  `egui_kittest` (`mockups/ui_mockup/`). Decided: single window with docked,
  collapsible, pop-out controller; 2×2 default with Graph in the 4th cell; tool
  shelf + cards; hooked cards show data flow; multiple overlay layers; Clone to
  compare. afni-io reviewed: Phases 1–5 done; p-value maths missing and will be
  added to afni-io (Milestone X).
- 2026-10-03 — M0 started. Crate created on eframe/egui 0.36.2 (sumaru's versions); mockups bumped to 0.36.2 with no code changes. Theme follows macOS; `~/.afniru` overrides it. Slice canvas is black in both themes, with a white option for publication figures. Graph view will use `egui_plot` 0.37 (needs egui ^0.36).
- 2026-10-03 — M0 nearly done: shell (menu, toolbar, status bar), dataset loading (CLI, dialog, drop), synthetic phantom, `~/.afniru` created on first run in `~/.afnirc` format, docs scaffold. Fixture `tests/fixtures/tiny2+orig` (4×5×6, 2 sub-bricks, TR 2). Open: `egui_kittest` shell snapshot (needs the shell split from `eframe::Frame`), then check the macOS theme switch by eye.
- 2026-10-03 — M1 done: one slice view with plane picker, slider, Page Up/Down, auto window, orientation letters, scale bar. Orientation is derived from the dataset matrix, so non-RAI data displays in standard orientation.
