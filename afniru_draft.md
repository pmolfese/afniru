# afniru: design plan

`afniru` is a Rust rebuild of the AFNI volume viewer: 2D MRI slices with AFNI's
viewing tools (underlay/overlay, thresholding, colormaps, clustering, sub-brick
selection, InstaCorr, …). It runs natively on macOS, Linux and Windows with no
XQuartz/X11/Motif. It is the volume counterpart to `sumaru` (the Rust SUMA
rebuild), and the two should feel like a pair.

This document describes **the direction we're going**: what afniru looks like,
how it's organized, and the rules the code follows. Progress is tracked
separately in [`afniru_ROADMAP.md`](afniru_ROADMAP.md). Both documents are meant
to be readable by a person or by another Claude instance picking up the work.

The UI direction is shown in rendered mockups in [`mockups/`](mockups/) (see
`mockups/README.md`). When this document says "image 07", it means
`mockups/afniru_07_instacorr.png`.

## Related projects

| Path | What it is | Relationship |
|---|---|---|
| `../afni-io/` | crate **`afni-io`** (imported as `afni_io`): pure-Rust HEAD/BRIK, NIfTI, GIfTI, NIML, 1D I/O | afniru's file I/O. Phases 1–5 of its roadmap are done (see "afni-io status") |
| `../sumaru/` | egui + wgpu SUMA rebuild (~54k lines) | Source of reusable logic: colormaps, threshold/fade, clustering, NIML talk |
| `../afni/src/` | AFNI C source | Reference behavior. Check AFNI semantics here, not from memory |
| `mockups/ui_mockup/` | Standalone egui crate that renders the mockup PNGs | **Reference only**, not production code. Look at it for layout, colors and widget ideas; don't copy it wholesale |

The long-term plan is a shared workspace:

```
afni-io     file formats, stat metadata (+ p-values, see below)   (../afni-io)
afni-core   color, overlay, threshold, cluster, coordinates       (extracted later from sumaru + afniru)
sumaru      surface viewer
afniru      volume viewer                                          (this repo)
```

`afni-core` doesn't exist yet. Until it does, afniru **copies** what it needs from
sumaru into clearly marked modules (see "Borrowing from sumaru"). Don't build the
workspace now.

---

## Ground rules

- **Do not `git add`, commit, or push.** The user runs all git commands. Report
  what changed and give the commands instead. This folder may not be a git repo
  yet; mention that, but don't `git init` without asking.
- `unsafe_code = "forbid"`, as in sumaru and afni-io.
- `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must
  pass.
- Keep GUI and logic apart. Anything with a right answer (slice extraction,
  coordinate math, thresholding, colormapping, clustering, compositing) lives in
  non-GUI modules with unit tests. The UI layer only reads state and returns
  `Action`s.
- When an AFNI behavior is reproduced, cite the C file and function in a doc
  comment (for example `/// Matches afni_func.c: AFNI_func_overlay`).

## Code readability (a first-class requirement)

afniru is meant to grow well beyond today's AFNI features, and to be approachable
by people who are **new to Rust** and to the codebase. Someone opening any file
should be able to tell, without reading the rest of the project, **what the file
is for and what each function does**. Treat this as a requirement, not polish.
Reviews should reject code that is correct but unreadable.

**Every file**
- Starts with a `//!` module doc of a few lines covering:
  - what the module is responsible for
  - what it deliberately does *not* do
  - which modules it talks to
  - for AFNI behaviors, which C source it follows
- Has one job. Target **under ~800 lines**, with a hard cap of 1,500. When a
  file grows past that, split it by responsibility, not by line count.
  (sumaru's `viewer/mod.rs` reached 11.8k lines. Don't repeat that.)

**Every public item** (`pub fn`, `pub struct`, `pub enum`, fields that matter)
- Has a `///` doc comment saying what it is or does, in plain words.
- States **units and conventions** where they matter: mm vs voxels, RAI vs RAS,
  0-based indices, inclusive/exclusive ranges.
- Says when it returns `None`/`Err` or can panic.
- Enable `#![warn(missing_docs)]` at the crate root so gaps show up.

**Inside functions**
- Comment the **why**, not the what. "Use the underlay grid because AFNI
  resamples the overlay onto it (afni_warp.c)" is useful; "loop over voxels"
  isn't.
- Prefer short functions with descriptive names over long ones with section
  comments. If a block needs a heading comment, it probably wants to be its own
  function.

**Naming**
- Use full words: `threshold`, `crosshair`, `sub_brick`, `underlay`.
- AFNI's short UI terms (`ULay`, `OLay`, `Thr`) are fine as **labels** in the UI,
  and in code when mirroring AFNI directly. Define them in the glossary.
- Put units or spaces in names where they could be confused: `pos_mm`,
  `ijk`, `xyz_rai`, `slice_index`, `opacity_0_to_1`.

**Plain Rust over clever Rust**
- Plain structs, enums and functions first.
- Use generics, trait objects, macros and explicit lifetimes only when they
  clearly pay for themselves. The intended extension point is the `Tool`
  trait (see "Tools are modules").
- Avoid deep `Option`/`Result` chains on one line. Name the intermediate values.
- Avoid `unwrap()` outside tests. Use `expect("why this can't fail")` when a
  failure really is impossible.

**Docs for newcomers** (live in `docs/`, kept current as part of each milestone)
- `docs/ARCHITECTURE.md`: a map of the source tree, one line per file, plus how
  data flows from file to pixels.
- `docs/GLOSSARY.md`: AFNI terms (sub-brick, ULay/OLay/Thr, RAI/LPI, pbar, NN1–3,
  +orig/+tlrc, …) for Rust developers, and Rust terms for AFNI users.
- `docs/ADDING_A_TOOL.md`: a step-by-step walkthrough, using a real tool as the
  worked example.

**Tests as documentation.** Name tests after the behavior they pin down
(`axial_slice_matches_3dmaskdump_order`). Keep fixtures tiny and explain where
they came from.

---

## UI direction

### Window

- **One window by default.** The controller is docked as a left sidebar; the
  view area takes the rest (images 01–03, 06).
- **Pop out.** The controller, or any single card, can pop out into its own
  window (egui viewport) for multi-monitor setups (image 04). Reasons for
  single-window-first: one thing for the window manager to handle, keyboard
  focus stays put, and the layout presets only work if afniru owns the space.
- **Rail.** The controller can collapse to a 60 pt icon rail. Clicking an icon
  opens that card as a pop-over (image 09).
- **Dark theme first**, with a light theme (image 05). Colors are defined once
  in `ui/theme.rs` as named tokens. Widgets never hard-code colors.

### View area

- **Layout switcher** in the upper right of the toolbar, with three modes: **1×3
  row**, **3×1 column**, **2×2 grid**. The 2×2 grid is the default. Its 4th cell
  is the **Graph** view (time series at the crosshair, AFNI-graph style, with
  stimulus blocks shaded). An "auto" choice that picks a layout from the
  window's aspect ratio is a candidate.
- **View cards.** Each view card has:
  - a header with the plane name and its mm coordinate (`z = 10.0 mm S`)
  - orientation letters on the image edges
  - a 20 mm scale bar and the zoom %
  - a slice slider with `index / max`
- **Plane colors.** Axial is blue, coronal green, sagittal orange. Each plane's
  slider uses its color, and the crosshair line for that plane is drawn in the
  same color in the other views. The crosshair has a small gap at the focus
  point so it doesn't hide the voxel.
- **Status bar.** Shows the grid, resampling mode, coordinate convention (RAI/LPI)
  and radiological/neurological, so the display's assumptions are always
  visible.

### Controller: tool shelf + cards

The controller is a **stack of tool cards**, and the **tool shelf** at the top
chooses which cards are in the stack (images 06–10). The shelf is a grid of
square tiles, each with an icon and a label:

| Tile | Card |
|---|---|
| Data | Datasets (pinned) |
| Overlay | Define Overlay |
| Cluster | Clusterize |
| InstaCorr | InstaCorr |
| Graph | Graph settings |
| Atlas | Atlas / Whereami |
| Draw ROI | Draw ROI |
| Montage | Montage |
| Xhair | Crosshair |
| Plugins | Plugins |

**Tile states**
- **Open:** gold icon, outline and filled pip.
- **Collapsed:** a ring pip.
- **Off:** dim. The card is hidden, but its settings are kept.

**Card headers** have, from left to right:
- a drag handle for reordering
- a collapse chevron
- the tool's icon and title
- a summary when collapsed, e.g. `NN1 · ≥40 vox · 4 clusters`
- pop-out (⬈) and close (×) buttons

Datasets is **pinned**: it shows 📌 instead of ×.

**Minimizing, from smallest to largest:**
1. Collapse a card to its summary line.
2. Close it (its state is kept).
3. Collapse the whole controller to the rail.
4. Pop out the controller or a card.

**Workspaces** are named tool sets, plus card order and fold states, e.g.
"Task fMRI", "Resting state", "ROI drawing". Switching workspace is one click.

**Tools change the views, not only the controller.** In image 07, InstaCorr:
- turns the overlay into the live r-map
- draws a seed ring in each view
- switches the graph to seed mean vs. voxel
- reports update latency in the status bar

### Hooked cards

Cards that depend on each other **attach**, and the connector shows **data flow**
(images 11–13):

- Clusterize attaches under a Define Overlay card ("clusters the threshold").
- When InstaCorr produces an overlay, that overlay attaches under InstaCorr
  ("feeds r-map"). This gives a chain: **InstaCorr → Overlay → Clusterize**.

How it works:
- **Attaching.**
  - Clicking a tile while its parent card is open attaches the new card under
    that parent.
  - Dragging a tile or card near a compatible parent shows a dashed drop slot.
  - An open card's footer lists what can attach to it: solid chips are
    attached, dashed chips are available.
- **Attached cards** are indented under the parent and linked by a connector
  line (the spine) with a ⛓ socket. A label on the socket says what passes
  along the link. Attached cards move together when dragged.
- **Folding.**
  - Collapsing a card folds only that card.
  - Clicking the ⛓ socket (or Alt-collapse on the parent) folds the **group**
    into one line that includes the child's summary
    (`|t| ≥ 3.10 · ⊞ 4 clusters`), with the child drawn as a stacked edge.
- **The rules live in code.** Each tool declares what it can attach under. This
  is the same dependency graph `Session` uses to recompute downstream results:
  moving the InstaCorr seed recomputes the overlay and then the clusters. The
  connectors draw that graph.
- **Not planned:** free-form node wiring (connecting any box to any box).
  AFNI's dependencies are few and fixed, so linear chains cover them.

### Multiple overlays

Classic AFNI has one overlay per controller. **afniru supports a stack of overlay
layers** per controller. Examples:
- a task t-map plus an InstaCorr r-map
- a stat map plus an atlas outline
- two contrasts in different colors

- **Each layer is its own Define Overlay card**, titled by its dataset
  ("Overlay 1 · stats.sub-01", "Overlay 2 · A_ICOR"). Each card has its own
  OLay/Thr sub-bricks, colormap, threshold, sign mode, range, opacity and
  visibility.
- **Hooks are per layer.** A Clusterize card attaches to a specific overlay
  layer, so each layer can be clustered on its own.
- **Layer order and visibility** are managed in the Datasets card:
  - a compact layer list with an eye toggle, a drag handle and an opacity value
    for each layer
  - top of the list = drawn on top
  - **"+ Add overlay"** creates a new layer and its card
- **Compositing** (`render/compose.rs`):
  1. Start from the windowed underlay.
  2. Apply each visible layer bottom-to-top: threshold, then colormap, then
     alpha blend at the layer's opacity.
  3. Draw optional per-layer outlines (AFNI's "B" boxed mode) last.
  The compositing is pure and unit tested.
- **Readout.** The crosshair card lists the value of every layer at the
  crosshair, with each layer's color swatch.
- **Graph.** A layer can optionally contribute a trace to the Graph view.

### Controllers A/B/… and **Clone to compare**

Like AFNI, afniru can have several controllers (A, B, C…), shown as tabs at the
top of the controller. Each controller has its own datasets, overlays, tools and
crosshair.

**Clone** is the new part. "Clone A → B" creates controller B with **the same
settings as A**: underlay, every overlay layer and its threshold/colormap,
tool cards and their state, crosshair, zoom and layout. The user then changes
one thing and sees immediately where the results differ.

- **Cheap.** Datasets live in a shared store (`Arc<Volume>`), so cloning copies
  settings, not voxel data.
- **Compare layout.** With two controllers visible, the view area splits:
  - by default, **A above B, each as a 1×3 row**, so the same plane lines up
    vertically
  - side by side and 2×2-per-controller are alternatives
- **Linked by default.** Crosshair, slice positions, zoom and pan are locked
  between A and B (AFNI's Lock). Each link is a toggle in the toolbar's Link
  menu.
- **Differences are visible.**
  - Any setting in B that differs from its clone source gets a "≠" marker in
    B's card, with A's value on hover.
  - A toolbar chip ("3 differences") lists the differing settings.
  - Each difference has "Sync from A" / "Push to B" buttons.
- **Later:** flicker mode (alternate A/B in the same view), a swipe divider,
  and a computed difference map (B − A) as an overlay.
- **Mockup still to do:** the compare layout and difference markers aren't in
  `mockups/` yet; the roadmap lists them.

---

## Technology choices

| Area | Choice | Why |
|---|---|---|
| GUI | **egui 0.36.x**, same version as sumaru | Immediate mode; widgets and code move between the two projects. Pin the exact version and upgrade it together with sumaru |
| App shell | **eframe** (wgpu backend) | Less boilerplate than raw winit + egui-wgpu. Has built-in multi-window viewports (for pop-outs) and file drag and drop |
| Slice rendering | **CPU-composited RGBA → egui texture** | AFNI slices are small (≤512²). Compositing underlay + overlay layers in Rust takes about a millisecond, is easy to test, and maps 1:1 onto AFNI's per-slice logic. A GPU path can come later if profiling calls for it |
| Icons | Start with egui's built-in glyphs; move to an icon font (e.g. `egui-phosphor`) | egui's default fonts lack some symbols (⋯ ⇄ ⊕ ⇧ ⌥) |
| File dialogs | `rfd` (as in sumaru) | Native open/save dialogs |
| CLI | `clap` derive (as in sumaru) | `afniru anat+orig func+orig`, plus flags later |
| Errors | `anyhow` in the app, `afni_io::Error` from I/O | As in sumaru |
| I/O | `afni-io = { path = "../afni-io" }` | Path dependency while both change |
| Headless UI tests / screenshots | `egui_kittest` (wgpu, snapshot) | Already proven by `mockups/ui_mockup`. Use it for layout regression snapshots |

Native menus and a dock icon are **not** goals. Menus are egui's in-window menu
bar.

**License:** afni-io is CC0 (US Government work) and sumaru is MIT OR Apache-2.0.
Decide afniru's license before the first public push. CC0 is the likely choice,
to match afni-io.

---

## afni-io status

Phases 1–5 of `../afni-io/afni-io_ROADMAP.md` are done, and all its tests pass.
afniru can use these features directly; no stopgap parsers are needed:

| afniru needs | afni-io provides |
|---|---|
| One reader for AFNI and NIfTI | `volume::read_any` → `Volume` with `dimensions`, `nvols`, `frame_f32(t)`, `value(i,j,k,t)`, `labels`, `stats` |
| Load only some sub-bricks | `volume::read_any_volumes` / `Brik::read_sub_bricks` (AFNI; NIfTI is read whole) |
| Voxel ↔ world coordinates | `ijk_to_dicom` (RAI, matches `3dinfo -aform_real`), `ijk_to_ras`, `obliquity`, `orientations`, `view`, `time_axis` |
| Stat metadata | `StatSpec` (23 AFNI stat kinds + params) from `.HEAD` STATSYM/STATAUX or the NIfTI AFNI extension; `ThresholdCurve` for FDR curves |
| Saving datasets (ROIs, clusters) | `Brik::write`, `Brik::new` (AFNI reads the output back voxel-for-voxel) |

**Known gaps:**
- **p-values.** afni-io parses `StatSpec` but has **no stat → p / p → stat
  maths**. Its roadmap had assigned that to `afni-core`. **Decision: add it to
  afni-io**, next to `StatSpec`, so every consumer (afniru, sumaru) gets one
  tested implementation now instead of waiting for `afni-core`. Suggested API:
  - `StatSpec::p_value(stat) -> Option<f64>` (two-sided where AFNI is
    two-sided)
  - `StatSpec::stat_for_p(p) -> Option<f64>`
  - an FDR `q` lookup through `ThresholdCurve`

  Port from AFNI's `thd_statpval.c` and `cdflib/`, aiming for all 23 kinds, and
  at least correlation, t, F, z, chi², beta, binomial, gamma and Poisson. Start
  from sumaru's `stats.rs`, which covers t, F, correlation, z and chi² and has
  reference-value tests. Check against AFNI's `cdf -t2p` / `ccalc`. This is a
  task in the afni-io repo, tracked in afniru's roadmap as a cross-repo item.
- **Partial NIfTI reads.** A NIfTI is read whole even when only some volumes are
  wanted. That's fine to start; large 4D NIfTIs (InstaCorr, Graph) would
  benefit later.
- **`frame_f32` copies.** afniru keeps its own `f32` copy of each sub-brick it
  uses (the internal `Volume` type below), so this is fine.

---

## Architecture

The guiding shape is: **Session (data, no egui) → Actions → UI reads Session**.
Tools are self-contained modules, so adding one doesn't touch the core.

```
src/
  main.rs                 CLI parsing (clap), eframe launch. Nothing else.
  app.rs                  eframe::App: owns Session + UiState, runs one frame:
                          draw UI → collect Actions → Session::apply → recompute

  session/                ALL application state; no egui types anywhere here
    mod.rs                Session: dataset store + controllers + active controller
    store.rs              DatasetStore: Arc<Volume> by id, so clones share voxels
    controller.rs         ControllerState: underlay, overlay layers, crosshair,
                          view layout, tool states, links. Clone = derive(Clone)
    overlay.rs            OverlayLayer: dataset, OLay/Thr sub-bricks, colormap,
                          threshold, sign mode, range, opacity, visible
    action.rs             enum Action { OpenDataset, SetUnderlay, AddOverlay,
                          MoveCrosshair, CloneController, AttachTool, ... }
                          plus Session::apply(action). Parseable from text later
                          (for AFNI -com style scripting)
    graph.rs              the tool dependency graph (hooks) and recompute order

  data/
    mod.rs                afniru's own Volume type (see below)
    load.rs               ADAPTER: afni_io::volume::Volume -> Volume. The only
                          module that touches afni-io's volume types
    synthetic.rs          generated test volumes (head phantom, gradients, fake
                          t-stat blobs) for development without real data
                          (start from mockups/ui_mockup/src/phantom.rs)

  geom/
    mod.rs                ijk <-> xyz, which axes each plane shows
    orient.rs             RAI/LPI display conventions, radiological vs neurological

  render/                 pure functions, all unit tested
    slice.rs              extract an axial/coronal/sagittal 2D slice from a Volume
    resample.rs           sample an overlay on the underlay grid (NN first)
    compose.rs            underlay window/level + overlay layers -> RGBA buffer

  analysis/               (borrowed from sumaru until afni-core exists)
    color.rs              colormaps, pbar definitions
    threshold.rs          threshold / sign mode / fade
    cluster.rs            voxel connected components (NN1/2/3), sizes, peaks

  tools/                  one folder per tool: the extension point
    mod.rs                the Tool trait, ToolId, the registry, attaches_to rules
    datasets/             mod.rs (settings + logic), card.rs (egui card)
    overlay/              ...
    clusterize/           mod.rs: ClusterSettings + run(); card.rs: the card
    instacorr/
    graph/                settings card; the plot itself is ui/graph_view.rs
    crosshair/
    atlas/  draw_roi/  montage/  plugins/      (later)

  ui/                     egui only; reads Session, returns Actions
    theme.rs              color tokens (dark + light), spacing, fonts
    shell.rs              title area, toolbar (layout switcher, Link, chips),
                          status bar
    view_area.rs          lays out 1x3 / 3x1 / 2x2 and the A/B compare split
    view_card.rs          one slice view: texture, crosshair, letters, slider
    graph_view.rs         the time-series view (4th cell)
    controller/
      mod.rs              controller sidebar: tabs, shelf, card stack, rail
      shelf.rs            the tool tiles
      card_frame.rs       shared card chrome: handle, chevron, summary, ⬈, ×
      hooks.rs            spine/socket drawing, attach chips, drop slot, folding
      rail.rs             collapsed rail + pop-over
    widgets/              pbar.rs, segmented.rs, chips.rs, readout.rs
```

### Tools are modules

A tool is a folder under `src/tools/` that implements one small trait. Adding a
tool means adding a folder and one line in the registry. Nothing in `app.rs`,
`ui/controller/` or `ui/view_card.rs` changes. This is the main extension point
for afniru's "beyond AFNI" plans, and the natural shape for plugins later.

```rust
/// One tool in the controller (Clusterize, InstaCorr, Atlas, ...).
/// Settings live in ControllerState; the trait only describes behavior.
pub trait Tool {
    /// Stable identifier, used in workspaces and saved sessions.
    fn id(&self) -> ToolId;
    /// Shelf tile label and icon, e.g. ("Cluster", "⊞").
    fn label(&self) -> &'static str;
    fn icon(&self) -> &'static str;
    /// Which tool this one hooks under, if any (Clusterize -> Overlay).
    fn attaches_to(&self) -> Option<ToolId> { None }
    /// The card body. Reads state, returns Actions; never mutates directly.
    fn card_ui(&self, ui: &mut egui::Ui, ctx: &ToolContext) -> Vec<Action>;
    /// One-line summary shown when the card is collapsed.
    fn summary(&self, ctx: &ToolContext) -> String;
    /// Optional drawing on top of a slice view (seed ring, brush, outlines).
    fn paint_view(&self, _plane: Plane, _painter: &egui::Painter, _ctx: &ToolContext) {}
}
```

Each tool's **logic** (e.g. `clusterize::run(&mask, &settings) -> Vec<Cluster>`)
lives in its `mod.rs`, free of egui and unit tested. The egui code lives in
`card.rs`.

### Internal `Volume` type: insulate from afni-io churn

afniru must not depend on afni-io's API shape outside `data/load.rs`:

```rust
/// A loaded dataset, in the form the viewer uses.
pub struct Volume {
    pub name: String,
    pub dims: [usize; 3],
    pub ijk_to_xyz: [[f64; 4]; 4],     // AFNI DICOM/LPS ("RAI") convention
    pub orient: [Orient; 3],           // per-axis orientation codes
    pub view: ViewType,                // Orig / Acpc / Tlrc
    pub tr_seconds: Option<f32>,       // from afni-io time_axis()
    pub sub_bricks: Vec<SubBrick>,
}

pub struct SubBrick {
    pub label: String,
    pub data: Vec<f32>,                // scaled values, len nx*ny*nz
    pub range: (f32, f32),             // finite min/max, cached
    pub stat: Option<afni_io::stat::StatSpec>,
}
```

Keep coordinates in AFNI's native LPS/"RAI" internally, because AFNI's readouts,
crosshair coordinates and `-com` commands use it. Convert to RAS only for display
(LPI option) or when talking to sumaru.

---

## AFNI behaviors to reproduce

Check each against the C source before implementing:

- **Grid:** images are drawn on the **underlay's** grid. Each overlay layer is
  resampled onto it (`afni_warp.c`, `AFNI_dataset_slice`). Default nearest
  neighbor, with a separate resample mode for the threshold sub-brick.
- **Image orientation:** AFNI's default is radiological (`AFNI_LEFT_IS_LEFT=NO`).
  Support both conventions and default to AFNI's.
- **Coordinate readout:** RAI by default; LPI optional (`AFNI_ORIENT`).
- **OLay / Thr sub-bricks:** per overlay layer. The color comes from one
  sub-brick and the threshold test from another. The test is `|thr| >= T` by
  default, with positive-only/negative-only options. Show `p` (and `q` when FDR
  curves exist) for `T`.
- **Pbar:** continuous or discrete panels, symmetric (±) or positive-only,
  "autoRange" from the overlay's range.
- **Alpha fading / boxed outlines:** the "A" and "B" buttons; sumaru has fade
  logic.
- **Clusterize:** after thresholding, label connected voxels (NN1 = faces,
  NN2 = +edges, NN3 = +corners), drop clusters below the minimum size (voxels or
  µL), bisided or one-sided. Report a table (size, peak, center of mass), and
  clicking a row jumps the crosshair. See `mri_clusterize.c` and `afni_cluster.c`.
  Validate against `3dClusterize`.
- **InstaCorr:** setup (dataset, ignore, blur, automask, despike, bandpass, seed
  radius), then a live correlation map as the seed moves. See `afni_instacorr.c`
  / `thd_incorrelate.c`.
- **Crosshair and keyboard:** arrow keys move the crosshair, Page Up/Down change
  the slice, and each view's slice follows the crosshair.
- **Lock** (between controllers): crosshair/slice/zoom linking, as in AFNI's
  Define Datamode → Lock.

## Borrowing from sumaru

Copy into `src/analysis/`, with a header comment noting the source file and the
date copied, so the later `afni-core` merge can reconcile them:

| sumaru file | Use in afniru | Adapting |
|---|---|---|
| `src/color.rs` | colormaps, label tables | Probably as-is |
| `src/overlay.rs` | `Threshold`, `ThresholdMode`, `FadeSettings`, `ClipMode` | Remove surface/node assumptions (`PerNodeColorCache`) |
| `src/cluster.rs` | `label_clusters` | Write a grid-specific flood fill for NN1/2/3. Don't allocate per-voxel neighbor `Vec`s for 256³ volumes. Size clusters by voxel count × voxel volume. Keep the `ClusterParams`/`ClusterSummary` shapes |
| `src/stats.rs` | **not copied**: p-values go into afni-io (see "afni-io status") | Use its tests as the seed for afni-io's tests |
| `src/afni.rs` | NIML talk to AFNI/SUMA | Later milestone |

Don't copy GUI code from `sumaru/src/viewer/`. Patterns are fine to imitate, but
the module layout here is deliberately different.

## Testing strategy

- **Unit tests** for everything in `geom`, `render`, `analysis`, each tool's
  logic, and `Session::apply`. Include `ControllerState` clone-then-diff tests.
- **Synthetic data** for most tests. Real data only in `tests/data/`, kept tiny
  (e.g. 8×8×8) and generated by AFNI commands recorded in `tests/data/README.md`.
- **Golden comparisons against AFNI** (C tools as the reference): `3dinfo`,
  `3dmaskdump`, `3dClusterize`, `cdf`/`ccalc`. If AFNI isn't on `PATH`, write
  the expected values into the test, with the command that produced them.
- **UI snapshots** with `egui_kittest`, for the shell, the layouts and the
  controller states. Compare with a tolerance, not pixel-exact.

## Open questions

1. License: CC0, like afni-io?
2. Should afniru speak AFNI's `-com` / plugout command language, so existing
   `@chauffeur_afni`-style scripts keep working? If yes, `Action` should be
   parseable from text early (the architecture leaves room for this).
3. Minimum data size to handle smoothly (e.g. 256³ × 300 time points?). This
   decides whether everything stays as `f32` in memory or sub-bricks load
   lazily.
4. Compare mode: should "Clone" also be offered per overlay layer (duplicate a
   layer with a different threshold), as a lighter version of a full A/B clone?
