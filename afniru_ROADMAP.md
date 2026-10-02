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
| 0 | Skeleton: crate, window shell, loading, docs scaffold | ⬜ |
| 1 | One slice: extraction, window/level, view card | ⬜ |
| 2 | Three linked views, layouts, coordinates | ⬜ |
| 3 | Controller shell: tool shelf, cards, rail, workspaces | ⬜ |
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
- [x] Direction for multiple overlay layers and Clone-to-compare (in the plan; mockups pending, see M5/M8)
- [x] `afniru_draft.md` rewritten as the design plan; this roadmap created
- [ ] Mockups still to do: multiple overlay layers (layer list + two overlay cards), Clone/compare layout with difference markers

## Milestone X — Cross-repo: p-values in afni-io ⬜

Lives in `../afni_rust` (its own rules apply; the user commits there too).
Needed by M4. See "afni-io status" in the plan.

- [ ] Add to afni-io's roadmap as a new phase (this reverses its earlier "p-values go to afni-core" decision)
- [ ] `StatSpec::p_value(stat)` and `StatSpec::stat_for_p(p)`, matching AFNI's sidedness
- [ ] Port from `afni/src/thd_statpval.c` + `cdflib/`. Priority kinds: correlation, t, F, z, chi², then beta, binomial, gamma, Poisson, then the rest of the 23
- [ ] Seed the tests with sumaru's `stats.rs` reference values; add values from AFNI's `cdf -t2p` / `ccalc`
- [ ] FDR q lookup through `ThresholdCurve`
- [ ] Optional: partial reads of NIfTI volumes (`read_any_volumes` for NIfTI)

## Milestone 0 — Skeleton ⬜

- [ ] `cargo new afniru` (edition 2024); egui/eframe pinned to sumaru's 0.34.3; `[lints.rust] unsafe_code = "forbid"`; `#![warn(missing_docs)]`
- [ ] `afni-io = { path = "../afni_rust" }`
- [ ] Module skeleton from the plan's Architecture section. Empty modules get a `//!` doc explaining what will go there
- [ ] `ui/theme.rs`: dark + light color tokens (take values from `mockups/ui_mockup/src/ui.rs`)
- [ ] `ui/shell.rs`: toolbar (breadcrumb, layout switcher placeholder), status bar, in-window menu bar (File → Open, Quit)
- [ ] Open datasets: `rfd` dialog, drag and drop onto the window, CLI paths (`clap`)
- [ ] `data/load.rs`: `afni_io::volume::read_any` → `Volume` (dims, matrix, labels, stats, TR)
- [ ] `data/synthetic.rs`: head phantom + fake t-map (from the mockup's `phantom.rs`)
- [ ] Status bar shows dataset name, dims, voxel size, sub-brick count
- [ ] `docs/ARCHITECTURE.md`, `docs/GLOSSARY.md`, `docs/ADDING_A_TOOL.md` (stub, filled in M3)
- [ ] `README.md` for the repo: what afniru is, how to build and run

## Milestone 1 — One slice ⬜

- [ ] `render/slice.rs`: axial/coronal/sagittal extraction on the underlay grid
- [ ] `render/compose.rs` (grayscale only): window/level, with auto 2–98% percentiles
- [ ] `ui/view_card.rs`: header (plane + mm coordinate), image with correct aspect from voxel sizes, zoom to fit, orientation letters, scale bar, slice slider with `index / max`
- [ ] Page Up/Down change the slice
- [ ] Tests: slice indexing for all three planes on a synthetic volume with known values; matches `3dmaskdump` order on a tiny real fixture

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

- 2026-10-02 — Design milestone. Mockups 01–13 rendered with egui 0.34.3 via
  `egui_kittest` (`mockups/ui_mockup/`). Decided: single window with docked,
  collapsible, pop-out controller; 2×2 default with Graph in the 4th cell; tool
  shelf + cards; hooked cards show data flow; multiple overlay layers; Clone to
  compare. afni-io reviewed: Phases 1–5 done; p-value maths missing and will be
  added to afni-io (Milestone X).
