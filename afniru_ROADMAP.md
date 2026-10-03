# afniru roadmap

Goal: a native, cross-platform Rust rebuild of the AFNI volume viewer that pays
homage to AFNI, feels modern, and is easy to extend. The design and the rules
the code follows are in [`afniru_draft.md`](afniru_draft.md). This file tracks
progress.

Status: ✅ done · 🚧 in progress · ⬜ not started

| # | Milestone | Status |
|---|-----------|--------|
| D | Design: mockups, plan, roadmap | ✅ |
| X | Cross-repo: afni-io p-values (stat ↔ p) | — not needed: afni-core has them |
| 0 | Skeleton: crate, window shell, loading, docs scaffold | ✅ |
| 1 | One slice: extraction, window/level, view card | ✅ |
| 2 | Three linked views, layouts, coordinates | ✅ |
| 3 | Controller shell: tool shelf, cards, rail, workspaces | ✅ |
| 3A | `afni_proc.py` processing rail and dataset provenance | ✅ |
| 4 | Overlay and threshold (one layer) | ✅ |
| 5 | Multiple overlay layers | ✅ |
| 6 | Hooked cards + Clusterize | ✅ |
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

## Milestone X — Cross-repo: p-values in afni-io — not needed

Superseded. The plan assumed `afni-core` did not exist and put p-values in
`afni-io`. `afni-core` now has them (`stats::p_value`, `critical_value`,
`threshold::transfer_threshold`, `fdr::q_value_for_threshold`), together with
the AFNI color scales, overlay evaluation, thresholds, compositing and volume
clustering, and `afni-io` re-exports its `StatSpec`. afniru depends on
`afni-core` directly (Milestone 4 onward). Nothing to do in `afni-io`.

Still open from this milestone's list, if wanted: partial reads of NIfTI
volumes (`read_any_volumes` for NIfTI).

## Milestone 0 — Skeleton ✅

- [x] `cargo new afniru` (edition 2024); eframe (wgpu backend) 0.36.2 + egui 0.36.2, matching sumaru's versions (sumaru: egui 0.36.2 / wgpu 30); `[lints.rust] unsafe_code = "forbid"`; `#![warn(missing_docs)]`
- [x] `afni-io = { path = "../afni-io" }` (git-ignored `.cargo/config.toml` patches afni-io's afni-core git dependency to `../afni-core`)
- [x] Module skeleton from the plan's Architecture section (`session/`, `analysis/`, `tools/`, `ui/controller/`, `ui/widgets/`, `ui/graph_view.rs`, `render/resample.rs`). Each empty module has a `//!` doc naming the milestone that fills it. Not scaffolded: `card.rs` inside each tool folder, and the later tools (atlas, draw_roi, montage, plugins)
- [x] `ui/theme.rs`: dark + light color tokens (from `mockups/ui_mockup/src/ui.rs`); follows the macOS appearance by default
- [x] `prefs.rs`: `~/.afniru` parser. Keys so far: `AFNIRU_THEME = System|Dark|Light`, `AFNIRU_CANVAS_BACKGROUND = Black|White`, `AFNI_LEFT_IS_LEFT`, `AFNI_SESSTRAIL`
- [x] `~/.afniru` is created with documented defaults on first run (`create_new`, never overwritten), in `~/.afnirc` format (`***ENVIRONMENT`, `//` comments). AFNI names keep their spelling: `AFNI_LEFT_IS_LEFT`, `AFNI_SESSTRAIL`; afniru-only keys are `AFNIRU_*`
- [ ] Preferences panel / write-back; more keys from `~/.afnirc` as the features that use them land (graph size, pbar, resample modes, …)
- [x] `ui/shell.rs`: toolbar (breadcrumb, layout switcher placeholder), status bar, in-window menu bar (File → Open, Quit)
- [x] Open datasets: `rfd` dialog, drag and drop onto the window, CLI paths (`clap`)
- [x] `data/load.rs`: `afni_io::volume::read_any` → `Dataset` (dims, voxel size, matrix, labels, TR). Stats (`StatSpec`) are not read yet; they arrive with M4
- [x] `data/synthetic.rs`: head phantom + fake t-map (from the mockup's `phantom.rs`); `--demo` opens the phantom
- [x] Status bar shows dataset name, dims, voxel size, sub-brick count (and TR, and `oblique`), plus grid and coordinate conventions on the right
- [x] Shell tests: `App::draw` is split from `eframe::App::ui` so it runs with a bare `Ui`; unit tests for open / failed open / CLI order / generation; `egui_kittest` snapshots of the empty shell, demo dark and light, the File menu open, and an error in the status bar
- [x] No blanket `#![allow(dead_code)]`: what is not used yet is marked `#[expect(dead_code, reason = ...)]`, so the compiler tells us when to remove it
- [x] `docs/ARCHITECTURE.md`, `docs/GLOSSARY.md`, `docs/ADDING_A_TOOL.md` (stub, filled in M3)
- [x] `README.md` for the repo: what afniru is, how to build and run

## Milestone 1 — One slice ✅

- [x] `render/slice.rs`: axial/coronal/sagittal extraction on the underlay grid, in screen orientation (oblique grids use the nearest cardinal axes; no resampling). `geom/orient.rs` maps voxel axes to R/A/S
- [x] `render/compose.rs` (grayscale only): window/level, with auto 2–98% percentiles (min–max fallback when they coincide, e.g. mostly-zero data)
- [x] `ui/view_card.rs`: header (plane picker + mm coordinate with letter), image with correct aspect from voxel sizes, zoom to fit, orientation letters, scale bar, slice slider with `index / max`, window controls with Auto. Nearest-neighbor texture (true voxels); the texture is rebuilt only when the slice, window or dataset changes
- [x] Page Up/Down change the slice
- [x] Tests: slice indexing for all three planes on a synthetic volume with known values (RAI, permuted and oblique grids, both display conventions); loaded voxel order matches `3dmaskdump` on `tests/fixtures/tiny2+orig`; `egui_kittest` snapshots of the card (`tests/snapshots/`)

Notes: the card's plane picker is a stopgap that M2 replaces with the three linked views. Display is on the dataset's own grid; the plane colors are already in `ui/theme.rs`.

## Milestone 2 — Three linked views, layouts, coordinates ✅

- [x] `ui/view_area.rs`: 1×3 / 3×1 / 2×2 (Graph placeholder in the 4th cell); switcher in the toolbar (painted icons). Cells are axial, sagittal, coronal, Graph. The toolbar also has R↔L and Xhairs toggles
- [x] Crosshair: click or drag to set it in all views; arrow keys move it one voxel on screen in the active card (the one last hovered or touched, outlined in gold); Page Up/Down change that card's slice; plane colors on sliders and on the lines that stand for each plane in the other views; gap and ring at the focus point
- [x] `geom/`: ijk ↔ xyz (`coords.rs`), RAI/LPI display option (`AFNI_ORIENT`), radiological/neurological toggle (`AFNI_LEFT_IS_LEFT`, and the toolbar button; default radiological)
- [x] Readout strip under the views: ijk, xyz with R/L/A/P/S/I letters, the signed coordinates in the chosen convention, value under the crosshair; window controls on its right. The status bar shows the conventions (`RAI · radiological`)
- [x] Tests: ijk↔xyz round-trips (including an oblique matrix); xyz matches `3dmaskdump -xyz` on `tiny2+orig` and on the oblique `obl+orig`; the oblique dataset's real matrix matches `3dinfo -aform_real`; pixel↔voxel inverses for every plane/orientation; key handling; `egui_kittest` snapshots of the three layouts in both themes

Decisions:
- **Oblique datasets** are shown on their cardinal grid (`ORIGIN`/`DELTA`), as `3dmaskdump -xyz` reports it; `Dataset::ijk_to_ras_real` keeps the true scanner matrix (`3dinfo -aform_real`) for later use (coordinates for sumaru, Whereami). Unverified against the AFNI GUI: check that its readout agrees on an oblique dataset.
- The crosshair is a voxel index, so it snaps to voxel centers.
- Window/level is one shared control for now; M3's Datasets card takes it over.
- The M1 single card is gone; `ui/view_card.rs` is now one plane's card.

## Milestone 3 — Controller shell ✅

- [x] `tools/mod.rs`: `Tool` trait (`card_ui`, `summary`, `pinned`, and the not-yet-used `attaches_to` and `paint_view`), `ToolId` (all ten shelf tools, with label, title, Phosphor icon and "planned in" text), and the registry `tools::tool()`. The `id()` method of the draft's trait is gone: the registry already maps `ToolId` → tool
- [x] `ui/controller/`: tabs row (A, a disabled + for Milestone 8), workspace menu, tool shelf (tile states open / folded / off / not built yet), card stack in a resizable left panel. `ControllerUi` is the sidebar's own state; cards never touch the session
- [x] Card chrome (`card_frame.rs`): drag handle (reorder, with an insertion line), chevron and title (click folds, Alt-click folds all), summary when folded, ⬈ placeholder (disabled, Milestone 10), × (hides; place and fold state kept), pin on the Datasets card
- [x] First tools: Datasets (ULay and sub-brick pickers, dataset summary) and Crosshair (editable x/y/z in the chosen convention with side letters, editable ijk, value). Editing sends `JumpToRas` / `MoveCrosshair`
- [x] Rail mode with pop-over cards (`rail.rs`): icons of the shown cards, the popover closes on Escape, ×, or a click outside; » expands
- [x] Workspaces (`workspace.rs`, pure data): named sets of card entries with order and fold state; the current one is edited live; Save as / Delete in the gear menu; saved with `eframe` storage together with the rail state; `normalize()` repairs older files when tools are added. Only a "Default" workspace ships (Datasets + Crosshair): "Task fMRI", "Resting state" and "ROI drawing" would look identical until their tools exist
- [x] `docs/ADDING_A_TOOL.md` written, using Crosshair as the worked example
- [x] UI snapshot tests (`egui_kittest`) for shelf states (open, folded, off, not built), folded cards, the rail, and the rail pop-over; click tests for folding a card and for the collapse/expand buttons; a save/restore test for workspaces

Also in this milestone:
- **Session** now exists (`session/`): `Session` (dataset store behind `Arc`, controllers, a generation counter), `ControllerState` (underlay, sub-brick, crosshair), and `Action` with `Session::apply`, which validates. The crosshair moved out of the view area into the controller. The views still edit the cursor directly while dragging (high frequency); tools only send actions.
- **Sub-brick choice** for the underlay is real: the views show the chosen sub-brick.
- **Icons**: `egui-phosphor` (regular weight only) is a dependency now; `ui/fonts.rs` installs it once.
- **Not done**: dragging tiles on the shelf to reorder them (the mockup mentions it); pop-out; more than one controller.
- The window/level controls stay in the strip under the views; the Datasets card could take them over later.

## Milestone 3A — `afni_proc.py` processing rail and dataset provenance ✅

This is a compact, independent inspector on the **right** side of the view
area. It must not replace or widen the normal tool controller on the left. The
rail explains how the currently viewed data moved through an `afni_proc.py`
pipeline and makes intermediate outputs easy to inspect. Parsing processing
provenance and assessing health are afniru application services, not
`afni-core` algorithms.

### Discovery and processing model

- [x] When afniru starts in an `afni_proc.py` results directory, detect the
  processing script and its generated inputs, outputs, review files, and QC
  artifacts automatically. Also allow the user to open a results directory
  explicitly; do not recursively search unrelated parent trees.
- [x] Parse the executed processing order rather than assuming one fixed
  pipeline. Recognize common stages such as inputs, slice timing, volume
  registration, anatomical alignment, template warp (MNI/TLRC), spatial
  smoothing, percent-signal scaling, and regression, while preserving optional,
  omitted, repeated, or custom blocks.
- [x] Define file-neutral application types such as `ProcessingRun`,
  `ProcessingStep`, `StepArtifact`, `HealthAssessment`, and `HealthEvidence`.
  Keep script/QC parsing in a provenance adapter so the UI does not depend on
  filenames or shell syntax.
- [x] Associate every step with zero, one, or several artifacts, including
  anatomical and EPI datasets, masks, transforms, motion/censor summaries,
  regressors, and reports. Record each artifact's role, space, path, existence,
  and whether `afni-io` can open it.
- [x] Preserve the source of every inferred fact (script option, generated
  command, dataset header, review value, or QC artifact) so the interface can
  explain its conclusions instead of presenting unexplained scores.

### Compact right-side rail

- [x] Add a narrow, collapsible right rail headed `Processing`, with the run
  name in a small subtitle. It should use roughly 14–16% of a normal desktop
  window and collapse to a slim icon strip so the image views retain priority.
- [x] Draw the actual steps from top to bottom as a subway-style line. Use
  neutral gray nodes for unselected steps and a blue node plus focus ring for
  the selected step. Selection color and health color are deliberately
  independent.
- [x] Put a compact health pill/icon at the far right of each step: green for
  good, amber for caution, red for failed/danger, and neutral gray for unknown
  or not assessed. Never use color as the only signal; include an icon,
  accessible label/tooltip, and screen-reader text.
- [x] Keep the default row to one line. Expanding the selected step shows only
  a concise reason and artifact count (for example `edge mismatch 2.4 mm` and
  `2 datasets · View`); full evidence and file lists open in a pop-over or
  detail card instead of permanently consuming view width.
- [x] Make the rail responsive: at narrow widths it collapses automatically or
  becomes a temporary drawer. It must not squeeze the left controller or make
  the three slice views unusably small.
- [x] Add keyboard navigation, focus styling, tooltips, and a legend for node
  selection versus health status.

### Health and dataset interaction

- [x] Base health on inspectable evidence rather than filename heuristics
  alone. Initial checks may include missing/unreadable outputs, inconsistent
  grids or spaces, motion/censor and outlier summaries, alignment/warp overlap
  evidence, regression warnings, and non-finite or empty datasets when those
  measurements are available. *(Done: missing/unreadable outputs, inconsistent grids across runs, mask Dice for alignment and warp, degrees of freedom, censor fraction, AFNI's pre-steady-state, 4095 and correlation warnings. Not done: outlier summaries, motion magnitude, non-finite/empty data, grids across spaces; see below.)*
- [x] Define the thresholds and provenance for every green/amber/red rule.
  Missing evidence yields `Unknown`, not a guessed `Good`; the detail view says
  exactly which check produced the state and why.
- [x] Aggregate step health deterministically from individual checks while
  retaining every check result. A red check must not be hidden by several
  green checks.
- [x] Clicking a step selects it and opens its concise health explanation.
  `View` presents the associated dataset or dataset pair and asks before
  replacing the current underlay/overlay selection. Preserve the crosshair in
  world coordinates when the new grid permits it.
- [x] Support steps with multiple useful comparisons, especially pre/post
  alignment, pre/post warp, pre/post smoothing, and model input versus residual
  output. Reuse Controllers A/B later for side-by-side comparisons rather than
  inventing a separate comparison system here.
- [x] Refresh the run model when outputs appear or change, without discarding
  the user's selected step or silently switching the displayed dataset.

### Verification

- [x] Add small committed fixtures for a typical complete pipeline, omitted
  blocks, custom order, multiple runs, missing outputs, malformed scripts, and
  partial/in-progress results.
- [x] Unit-test processing-order recovery, artifact association, health
  aggregation, and selection-to-dataset actions independently of egui.
- [x] Add `egui_kittest` snapshots for the compact rail, selected/expanded
  step, all four health states, collapsed rail, narrow-window drawer, and long
  or custom step labels.
- [x] Document supported `afni_proc.py`/QC inputs and every health rule in
  `docs/PROCESSING_RAIL.md`; add the new modules and data flow to
  `docs/ARCHITECTURE.md`.

Decisions and gaps:
- Code: `src/processing/` (types, `script.rs`, `discover.rs`, `review.rs`, `health.rs`, `model.rs`) and `ui/processing_rail.rs`; `docs/PROCESSING_RAIL.md` lists the inputs and every rule with its threshold. The thresholds are afniru defaults based on AFNI's review guidance, not AFNI standards, and are constants in `health.rs`.
- Fixtures: real `afni_proc.py` scripts (`tests/fixtures/processing/*/proc.sub-01`, from `make_fixtures.sh`) with results trees built at test time; there is no real results directory on the development machine, so **the `out.ss_review` format is taken from `gen_ss_review_scripts.py`'s echo lines and has not been checked against a real run** (and neither have the warning-file formats). Please try a real results directory.
- Evidence kinds in use: generated command (script line), dataset header, review value, QC artifact, file system. `SourceKind::ScriptOption` exists but nothing uses it yet (the censor limits in the `afni_proc.py` command could be shown beside the censor check).
- Not done: non-finite/empty-dataset checks; outlier and motion-magnitude summaries; NIfTI outputs are existence-checked only; multiple `out.ss_review` files; the QC HTML is not read.
- `View` offers the step's outputs and the earlier output of the same kind; true side-by-side comparison waits for Controllers A/B (Milestone 8).
- The run lives in `App`, not in `Session`; the Processing rail's collapsed state is saved with the app.

## Milestone 4 — Overlay and threshold (one layer) ✅

Built on `afni-core` (no copying from sumaru, no Milestone X): see "Decisions" below.

- [x] `session/overlay.rs`: `OverlayLayer` (dataset, OLay/Thr sub-bricks, color scale, ± or +, range or auto, threshold, opacity, visible, A, B) and `OverlayChange`, applied by `Session::apply(Action::Overlay(..))`
- [x] `render/resample.rs`: NN resampling of the overlay onto the underlay grid (NaN outside; ties on a voxel edge go to the higher voxel); done once per overlay/sub-brick choice, not per threshold change
- [x] ~~`analysis/color.rs` + `threshold.rs` copied from sumaru~~ → not needed: `afni-core` provides `AfniColorScale`, `Threshold`, `FadeModel` and `evaluate_rows`; `render/overlay.rs` builds the `OverlaySpec`. `analysis/color.rs` and `threshold.rs` stay empty placeholders (clusters in Milestone 6 will use `afni-core::volume_cluster` the same way)
- [x] Define Overlay card (`tools/overlay`): pbar with ticks and threshold marks and a vertical threshold slider beside it, the nine AFNI color scales, ± / +, range + auto, A (fade) and B (boxed outline) toggles, opacity, show, threshold entry, `p` (editable: type a p-value to set the threshold) and `q` (from the FDR curve) under it, the statistic and its sidedness. Datasets card gained OLay, OLay sub and Thr sub pickers; Crosshair shows the OLay and Thr values
- [x] Tests: known-input colors (extremes, threshold edge, positive-only, fixed range, opacity, A fade, B outline, NaN, separate Thr sub-brick); compositing over gray; resampling (identity, shift, finer grid, flip, singular); threshold `p` matches `cdf -t2p` and `cdf -p2t`, and `q` matches `fdrval`, on the committed `stat+orig` (`tests/fixtures/README.md` has the commands); `egui_kittest` snapshots of the card and views (default, positive + boxed, fade + range + opacity, no overlay, real stat dataset with p and q, light theme)

Decisions:
- **`--demo`** now loads the phantom with a fake t-map overlay. **`afniru anat func`**: the second dataset is the overlay.
- **Starting settings.** A new overlay layer starts as AFNI does: ±, auto range, `AFNI_COLORSCALE_DEFAULT` (new preference, default `Reds_and_Blues_Inv`, AFNI's own; first `Spectrum:red_to_blue`, switched after M5B), **threshold 0**, OLay sub-brick #0 and Thr the first statistic sub-brick. Changing a layer's dataset or sub-bricks leaves its threshold alone. (An earlier version started statistics at two-sided p = 0.001; that was dropped to match AFNI.) `--demo` thresholds its t-map at 3.1 so it looks like a result.
- **Sidedness.** ± uses a two-sided p when the statistic supports it (t, z, correlation); positive-only, F, χ² and the like are one-sided. The card shows which.
- **Zeros** are not colored (AFNI's `AFNI_OVERLAY_ZERO = NO`); NaN is never drawn.
- **Color range** is `[-R, R]` (±) or `[0, R]` (+), `R` = the largest absolute value in the whole OLay sub-brick unless fixed. The scale is continuous (256 steps); AFNI's discrete panes are not offered.
- **The threshold marks on the pbar** are drawn only when OLay and Thr are the same sub-brick (same scale).
- **Not done:** `Reds_and_Blues_Inv` (the mockup's scale) is not an AFNI scale in `afni-core`; discrete pbar panes; setting the threshold by q; MDF; threshold-sub-brick resampling mode (always NN); several overlays (Milestone 5).

## Milestone 5 — Multiple overlay layers ✅

- [x] Mockup first: done with the real app instead of the standalone mockup crate: the `egui_kittest` renders of the app (`overlay_two_layers.png` and friends) served as the mockups, since the controller, cards and layer list are the real code
- [x] `ControllerState.overlays: Vec<OverlayLayer>` (bottom first, each with a `LayerId`; the id is the layer's number in the interface); "+ Add overlay" (a menu of datasets) in the Datasets card; one Define Overlay card per layer, titled "Overlay N · dataset", each with its own dataset and OLay/Thr pickers. The `Tool` trait gained `instances()`: a tool can have several cards (workspaces still hold one entry per tool; the cards of a multi-card tool sit together at the tool's place and fold independently, not saved)
- [x] Layer list in the Datasets card, top layer first (top = drawn on top): drag handle, eye, a swatch of the color scale, name, opacity %, remove. Actions: `AddOverlay`, `RemoveOverlay`, `MoveOverlay`, `Layer(id, change)`
- [x] Compositing bottom to top through `afni-core`'s `composite_layers`; hidden layers are skipped; layers with B (boxed) are drawn as outlines after all the filled layers, so outlines are always on top. Each layer's OLay/Thr are resampled once and cached per layer
- [x] The Crosshair card lists every layer's OLay and Thr at the crosshair, top first, each with a swatch of the color it is drawn in there (a ring when it is not drawn)
- [x] Tests: two-layer composite order and opacity (`compose`), the order of planes with outlines last and hidden layers skipped (`view_card`), session actions for add/remove/move/change (ids not reused, changes hit only the named layer), the drop-position arithmetic, clicks on the eye/trash/title/Add overlay, a simulated drag that restacks, and snapshots (two layers, one hidden and one folded, a boxed bottom layer, the readout)

Decisions:
- **Layer cards have no ×.** The cards of a multi-card tool are added and removed where they are managed (the layer list); the Overlay tile hides or shows all of them. The eye hides a layer; the trash removes it.
- **Command line.** `afniru anat f1 f2`: `f1` and `f2` become layers in that order, `f2` on top.
- **Workspaces** keep one Overlay entry; per-layer fold state is not saved because layers belong to the session.
- **Not done:** a layer cannot yet contribute a trace to the Graph (Milestone 7); no per-layer blend mode (only "over").

## Milestone 5B — Mask layers (rules over overlays) ✅

Added after M5 at the user's request: a layer can switch from a continuous color map to
an **on/off mask**, where every "on" voxel has the same color, and the rule can be a
`3dcalc` expression. Several layers then give "where A is", "where B is" and "where both
are".

- [x] **Engine in `afni-core::calc`** (new Phase 12 there; the user commits that repo): `Expr::parse`/`eval`, ported from AFNI's `parser.f`, ~190 cases checked against `1deval` (`tests/data/conformance/calc.ref`), differences in `docs/DIFFERENCES_FROM_AFNI.md` §13. Core function set; other AFNI functions are rejected by name.
- [x] `OverlayLayer.as_mask` + `mask: MaskSettings { rule, color }` (kept when switching back to a color map) and `bindings: letter → Binding`. Rule = `Threshold` (the layer's own, zeros never count) or `Expression(text)`; on where the result is not zero (NaN is off)
- [x] **What a letter can be** (`Binding`): this layer's OLay or Thr; any dataset sub-brick (resampled onto the underlay grid); another layer's **drawn/on-off result** (0/1) or its OLay value (hidden layers included); or a built-in `x y z` (mm, DICOM order) / `i j k` (voxel indices). New layers start with `a` = OLay and `b` = Thr
- [x] Session rules: a binding must name an existing dataset and sub-brick, or an existing *other* layer that does not (even indirectly) read this one, so cycles are refused; removing a layer drops the bindings that read it
- [x] `render/layers.rs` evaluates the whole stack at a list of voxels (a slice, or one crosshair voxel), memoizing layers that others read; `render/mask.rs` applies the rule; problems (parse error, unbound letter, missing dataset or layer) are reported and draw nothing
- [x] Card: Show as [Color map | Mask], the mask color, "is on where [threshold | rule]", the rule box with the parse error under it, one row per letter used with a menu of what it can stand for; the layer list shows a mask's color; the Crosshair card's swatch is the color actually drawn there (mask color when on)
- [x] Tests: engine conformance (afni-core); stack evaluation (A, B, both, hidden source, a source above the reader, values, coordinates, sub-brick, every kind of problem, cycle guard, NaN, one voxel); session binding validation; app-level "A / B / both" probes at chosen voxels; typing a rule through the card; snapshots of three masks, only the conjunction, a rule with a mistake, an unbound letter, the threshold-mask card

Decisions and gaps:
- **Relational operators.** 3dcalc has none (`step(a-3)`, not `a>3`). The rule language was 3dcalc's exactly at first; afni-core then added `< <= > >= == != && || ! ?:` as a clearly marked extension (DIFFERENCES C-11), so both forms work.
- **Core function set only** (arithmetic, math, mask functions, variable-argument statistics). `erf`, random numbers, the `fico_*` conversions and the rest give "not implemented" naming the function.
- **A rule layer still has a dataset** (its OLay/Thr pickers); a pure conjunction can use any. A "rule-only" layer without one would be a cleaner model.
- **Missing data:** a voxel outside an overlay's field of view reads as 0 in rules (AFNI would resample outside as 0 too); AFNI's own `step(NaN)` is 1, which afniru avoids by never binding NaN.
- The A (fade) button does not apply to masks. B (boxed) keeps the fill and adds a solid outline drawn after the filled layers (it used to replace the fill, which also made A disappear).
- Clusterize (M6) will take a mask layer as input without further work.

## Milestone 6 — Hooked cards + Clusterize ✅

- [x] `session/graph.rs`: the tool dependency graph (`parent`, `children`, `downstream`), read from `Tool::attaches_to`; Clusterize hooks under Overlay
- [x] `ui/controller/hooks.rs`: spine and ⛓ socket with the data-flow label ("clusters the threshold"), the hooked card indented under its parent's card of the same layer, group fold by clicking the socket (the group becomes one line with the child's summary and a stacked edge; Alt-click on a header folds every group)
- [x] `ui/widgets/chips.rs`: attach chips in a card's footer (solid gold = hooked, dashed = available); a chip hooks and unhooks the tool on that layer
- [x] `tools/clusterize/`: `compute.rs` (the flood fill is `afni_core::volume_cluster`; this decides its inputs from a layer), `engine.rs` (when to rerun), the card (NN 1/2/3, min size in voxels or µL, bisided, "only clusters")
- [x] Cluster table: rank, voxels, peak, where; the cluster under the crosshair is gold; clicking a row jumps to its peak (µL on hover)
- [x] Overlay restricted to the surviving clusters ("only clusters")
- [x] Golden tests against `3dClusterize` on `tests/fixtures/clust+orig` (`tests/fixtures/make_clusterize_fixtures.sh` regenerates the reports): NN1/2/3, bisided, positive-only, two-sided, min size in voxels and µL; sizes, centers, peaks and their positions, means
- [x] Tests also: engine caching (colors and opacity do not recluster, thresholds do, the mouse-down hold), mask layers, a different underlay grid, `restrict` in the layer evaluation, session validation, and app-level hooking through the chip and the tile, "only clusters", the row click, three snapshots

Decisions and gaps:
- **Hooks are per layer.** The settings live on the layer (`OverlayLayer::cluster`, `None` = not hooked), so removing a layer removes its clusters, and two layers can be clustered differently. The Clusterize tile hooks the top layer when nothing is hooked; the Overlay card's chip hooks any layer. Hooking from a chip turns the Clusterize tile on. Closing a hooked card (×) unhooks it (its settings are dropped); turning the tile off only hides the cards.
- **Color-map layers are clustered on the overlay dataset's own grid**, as AFNI's Clusterize does, then the surviving voxels are mapped to the underlay grid (nearest neighbor) for "only clusters". **Mask layers are clustered on the underlay grid**, where their rule is evaluated; they have no peak, so the table shows the center of mass.
- **Defaults** (afniru's, not AFNI's): NN1, at least 10 voxels, bisided, "only clusters" off.
- **Min size in µL is a real volume.** `3dClusterize -clust_vol V` reads V as a *number of voxels*; afni-core and afniru do not (see afni-core's header).
- **Clustering runs on a background thread** once the mouse is released (`engine.rs`); until it finishes the card keeps the old clusters marked "(updating)". Only a mask layer's rule is still evaluated on the interface thread (it needs the views' caches).
- The table shows the first 200 clusters. Click a heading to sort (again to reverse); **Copy** puts the table on the clipboard and **Save…** writes it (tab separated, with a line saying what was clustered); both use the current coordinate convention.
- **Dragging the Cluster tile** over the cards shows a dashed drop slot under each layer card that has no Clusterize; releasing on it hooks it there. The tile's click still hooks the top layer.
- The Overlay card's chips list only Clusterize today; "Histogram" and "Atlas labels" in the mockup come with their tools.
- A hooked card cannot be dragged away from its parent; it follows it.

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
- [ ] Publication figures: with a white canvas the image's own background voxels (zeros) stay black. Needs an option to draw background (or below-window) voxels as transparent/white
- [ ] Compare extras: flicker, swipe divider, B − A difference overlay
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
- 2026-10-03 — M2 done: three linked views with crosshair, 1×3 / 3×1 / 2×2 layouts, RAI/LPI and radiological/neurological, readout strip. Fixtures `obl+orig` (oblique) and `*_maskdump_xyz.txt` added. Found that `3dmaskdump -xyz` uses the cardinal grid for oblique data (see M2 decisions).
- 2026-10-03 — M0 closed: shell snapshot tests, module skeleton, dead-code allow removed (unused items are `#[expect(dead_code)]` with the milestone that needs them). The status bar now shows the grid code and `oblique`; the readout shows the sub-brick label; the breadcrumb's hover shows the file path.
- 2026-10-03 — M3 done: controller sidebar (shelf, cards, rail, workspaces), `Tool` trait with Datasets and Crosshair, `Session` and `Action`, Phosphor icons, persisted workspaces.
- 2026-10-03 — M3A done: Processing rail (right), `processing/` services, health rules with stated thresholds, View-with-confirmation, refresh, narrow-window drawer; `Session::apply(SetUnderlay)` now keeps the crosshair at the same world position when the new grid covers it. Real `afni_proc.py` scripts as fixtures.
- 2026-10-03 — M4 done, on `afni-core` (p-values, FDR q, AFNI color scales, overlay evaluation, thresholds, compositing). Milestone X is not needed. New fixture `stat+orig` with AFNI golden values for p and q; new preference `AFNI_COLORSCALE_DEFAULT`.
- 2026-10-03 — M5 done: a stack of overlay layers with a layer list, one card per layer, bottom-to-top compositing with outlines last, per-layer readout swatches. New layers start at threshold 0, as in AFNI.
- 2026-10-03 — M5B: mask layers with `3dcalc` rules. The expression engine was written in `afni-core` (`calc`, Phase 12) and checked against `1deval`; afniru's session, evaluation and card build on it. Reading `parser.f` showed AFNI's `absextreme` never runs in practice (see afni-core's DIFFERENCES C-5).
- 2026-10-03 — M6 done: Clusterize hooked under Define Overlay (per layer), with the spine/socket UI, attach chips and group fold. Clusters match `3dClusterize` (new fixture `clust+orig` and AFNI's reports). Operators `<`, `&&`, `?:` etc. were added to `afni-core::calc` for mask rules, and the default overlay scale is now AFNI's own `Reds_and_Blues_Inv`.
- 2026-10-03 — Background loading (`loader.rs`): datasets read on worker threads, applied in the order asked for, with name/size/elapsed shown; folders list their datasets (`AFNIRU_FOLDER_BROWSER`), loading only what is picked; replaced or removed datasets are freed. B keeps the fill. The AFNI logo is the window/dock icon.
