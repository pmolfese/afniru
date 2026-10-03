# afniru

A native, cross-platform Rust rebuild of the AFNI volume viewer: 2D MRI slices
with AFNI's overlay, threshold and cluster tools, in a modern single-window UI.
Sister project of [sumaru](../sumaru) (the SUMA rebuild). Early development:
see [`afniru_ROADMAP.md`](afniru_ROADMAP.md) for status and
[`afniru_draft.md`](afniru_draft.md) for the design.

## Build and run

Needs Rust 1.99+ and the sibling checkouts `../afni-io` (and `../afni-core`,
via the git-ignored `.cargo/config.toml` patch; remove that file to fetch
afni-core from GitHub instead).

```sh
cargo run --release -- --demo                 # built-in phantom, no data needed
cargo run --release -- anat+orig.HEAD         # AFNI dataset
cargo run --release -- T1.nii.gz func.nii.gz  # NIfTI; first file is the underlay, second the overlay
```

You can also use File ▸ Open…, or drop a file on the window.

**Folders.** Give a folder instead of (or as well as) datasets, or use File ▸ Open
folder…, and the AFNI (`.HEAD`/`.BRIK`) and NIfTI (`.nii`, `.nii.gz`) datasets in it
become choices in the dropdowns: the **ULay** dropdown in the Datasets card
picks the underlay, the **Dataset** dropdown in a Define Overlay card picks that
layer's dataset, and the **+** beside it (or **Add overlay**) adds another
overlay layer. Datasets already loaded are listed first; a long list gets a
filter box. Only the names are read; a dataset is loaded when you pick it (or name it on
the command line), and a dataset you replace or remove is dropped from
memory. `AFNIRU_FOLDER_BROWSER = NO` in `~/.afniru` turns the listing off. A
folder that is an `afni_proc.py` results directory also opens in the
Processing rail.

**Loading** happens in the background, so the window stays usable while a big
dataset (or a network drive) is read. The status bar and the Datasets card show
each dataset being read with its size and the time so far, and a button to stop
waiting. Datasets are applied in the order you asked for them, however fast
each reads.

Click or drag in a view to move the crosshair; arrow keys move it one voxel in the view under the mouse; Page Up/Down change that view's slice. The toolbar switches layouts (1×3, 3×1, 2×2), swaps radiological/neurological, and hides the crosshair lines.

## Controllers, compare and link

A controller is one AFNI controller: its own underlay, overlay layers, tools,
crosshair and Graph. The tabs at the top of the controller panel are A, B, …;
the **+** clones the active controller into a new one (same data, copied
settings), and the view area switches to **compare**: A and B side by side.
Change B's threshold, color scale or overlay and see both at once. Click a tab
or click in a half to work on that controller; right-click a tab to copy it over
another or close it. The **Link** button in the toolbar links the crosshair
(and so the slices) and the zoom, so scrolling through A scrolls B to the same
place in the world even if their grids differ. The **A ≠ B** chip lists what
differs and can make one like the other.

**Zoom:** Ctrl+scroll (or a pinch) zooms around the pointer; scroll pans;
double-click shows the whole image.

## The Graph

The fourth cell of the 2×2 layout plots the time series at the crosshair
(click or drag in it to change the time point shown). The Graph card chooses the
dataset (any dataset with several time points, not only the underlay), a **Fit**
drawn over it (any dataset with the same number of time points), a **matrix** of
1×1, 3×3 or 5×5 neighboring voxels (click one to move the crosshair there), how
many time points to **ignore**, a **detrend**, **% of mean**, and a **stimulus**
(a `.1D` column, shaded where it is on). Drag a box in the graph to zoom, scroll to pan, double-click or **Full course** to see everything; hovering shows the time point and value. The footer gives the mean, standard
deviation, range, the value at the current time point and, with a fit, R².

## Slice numbers and saving images

Right-click an image for its menu. **Slice number** draws the slice number on
all three views; **Number position** (the four corners) and **Number size**
(small to extra large) set where and how big, and also turn it on. (To start
that way, set `AFNIRU_SLICE_NUMBER`, `AFNIRU_SLICE_NUMBER_CORNER` and
`AFNIRU_SLICE_NUMBER_SIZE` in `~/.afniru`.)

The same menu saves PNG files: **Save this slice…**, **Save the three views**
(one row, one column, a 2×2 grid, or three separate files), and **Montage and
more options…**, a dialog for a montage of slices of one plane (rows, columns,
first and last slice, step, with a "fit" button) and for the look of the files:
the size (pixels per voxel, ×1 to ×8; voxels are always square blocks, never
smoothed), orientation letters, the slice number, and the crosshair. Pictures
use the underlay window, every visible overlay layer, and the radiological or
neurological display you are using; the background follows
`AFNIRU_CANVAS_BACKGROUND` (white for publication figures). The Graph can be
saved too, alone or with the three views (as a fourth picture in a row or column, or in its cell of the 2×2 grid).

## The overlay

Every dataset after the first on the command line is drawn in color over the
underlay as an **overlay layer** (later ones on top), resampled onto the
underlay's grid (nearest neighbor). The Datasets card lists the layers, top
first, with an eye, opacity, a handle to drag them into a new order, and
**Add overlay**. Each layer has its own **Define Overlay** card with the color bar and a
threshold slider beside it, the color scale (`AFNI_COLORSCALE_DEFAULT` sets the
starting one), ± or positive-only, AFNI's **A** (fade values below the
threshold) and **B** (box the suprathreshold regions: they stay filled, with A's fade if on, and get a solid outline), the color range
(automatic or fixed), opacity, the threshold, and for statistics the **p** and
the FDR **q** of the threshold. Type a p-value to set the threshold from it.
A new layer starts at threshold 0, as in AFNI. The Crosshair card lists every
layer's value at the crosshair with a swatch of the color it is drawn in.
The statistic comes from the dataset header (`3dinfo` shows it).

### Clusterize

Each layer's card ends with an **ATTACH** row. The **Cluster** chip hooks a
Clusterize card under that layer (the Cluster tile hooks the top layer): the
card is indented under the layer's, joined by a spine whose ⛓ socket folds the
pair into one line. Clusterize groups the voxels that pass the layer's
threshold by how they touch (**NN** 1 faces, 2 faces and edges, 3 corners too),
drops clusters smaller than **min** (voxels or µL), and lists the rest: size,
peak and where it is. A click on a row jumps to the peak; the cluster under the
crosshair is gold. **bisided** clusters positive and negative values
separately; **only clusters** draws just the surviving voxels. The numbers match
`3dClusterize` (checked on a committed dataset). A mask layer is clustered too.
Dragging a threshold keeps the old clusters until you let go.

### Masks and rules

A layer can be shown as a **mask** instead of a color map: every voxel is on or
off, and every "on" voxel is one color. The mask is on where the layer's
threshold passes, or where a **rule** is not zero. A rule is a `3dcalc`
expression (`step(a-3)*step(b-2)`, `within(a,2,4)`, `ifelse(…)`, …) or, as an
afni-core extension, a C-style one (`a>3 && b<=2`, `a>0 ? a : 0`); its letters
stand for this layer's OLay or Thr, any dataset's sub-brick, `x y z` / `i j k`,
or **another layer** (where it is drawn, or its value). So one layer can show
where A is, another where B is, and a third `a*b` where both are, with A and B
hidden if you like. Rules use 3dcalc's language, so `step(a-3)` works as in AFNI;
`a>3`, `<=`, `==`, `!=`, `&&`, `||`, `!` and `c ? x : y` are added (AFNI itself
rejects them). See `afni-core`'s `docs/DIFFERENCES_FROM_AFNI.md` §13.

## The controller

The left sidebar holds tool cards. The shelf of tiles turns cards on and off
(gold = open, ring = folded, dim = off; the dimmest tiles are tools not built
yet). Click a card's title to fold it, drag its handle to reorder, × hides it.
`«` collapses the sidebar to an icon rail; clicking an icon opens that card as
a pop-over. Arrangements are saved as workspaces (the gear menu).

## The Processing rail

For an `afni_proc.py` run, the rail on the right shows the pipeline steps with
their health. afniru finds the run from the working directory or the folder of
a file you open; or pass a results directory (`afniru sub-01.results`), use
File ▸ Open results directory…, or drop it on the window. See
[`docs/PROCESSING_RAIL.md`](docs/PROCESSING_RAIL.md).

## Settings

On first run afniru writes `~/.afniru`, a commented file in AFNI's `~/.afnirc`
format with every setting at its default. The theme follows macOS unless
`AFNIRU_THEME` says otherwise.

## Development

```sh
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and
[`docs/GLOSSARY.md`](docs/GLOSSARY.md).
