# Adding a tool

A tool is a card in the controller plus a tile on the shelf. Adding one touches
its own folder, one line in `tools/mod.rs`, and nothing else: not `app.rs`, not
`ui/controller/`, not the views. The **Crosshair** tool
([`src/tools/crosshair/mod.rs`](../src/tools/crosshair/mod.rs)) is the worked
example below; **Datasets** is a second one with pickers and a pinned card.

## The pieces

| What | Where |
|---|---|
| Name, label, icon, "planned in" text | `ToolId` in `src/tools/mod.rs` (already lists all ten shelf tools) |
| The behavior | `impl Tool for MyTool` in `src/tools/<name>/mod.rs` |
| Registration | one line in `tools::tool()` |
| Things the tool can change | `session::Action` (+ a line in `Session::apply`) |

A tile for a tool without an implementation is shown dimmed with a "planned
for Milestone N" tooltip, so most shelf entries already exist.

## Step by step (Crosshair)

1. **Name it.** `ToolId::Crosshair` already has its label (`Xhair`), title
   (`Crosshair`) and Phosphor icon (`CROSSHAIR`). A new tool adds a variant
   to `ToolId`, `ToolId::SHELF`, and the `match`es in `label`, `title`,
   `icon` and `planned_in`.
2. **Write the tool.** A unit struct and `impl Tool`:

   ```rust
   pub struct CrosshairTool;

   impl Tool for CrosshairTool {
       fn card_ui(&self, ui: &mut Ui, cx: &ToolContext) -> Vec<Action> { /* ... */ }
       fn summary(&self, cx: &ToolContext) -> String { /* one line when folded */ }
       // Optional: pinned(), attaches_to(), paint_view()
   }
   ```

   - **Read** everything from `ToolContext`: the theme, the session, the
     active controller (underlay, crosshair), the underlay dataset, the
     coordinate convention, the value under the crosshair.
   - **Never mutate** the session. Return `Action`s. Crosshair returns
     `Action::JumpToRas` when you edit a coordinate and `Action::MoveCrosshair`
     when you edit the voxel index.
   - Take colors from `cx.theme` or `ui::theme` constants, never hard-coded.
3. **Register it.** In `tools::tool()`:

   ```rust
   ToolId::Crosshair => Some(&crosshair::CrosshairTool),
   ```
4. **Need a new action?** Add a variant to `session::Action` and handle it in
   `Session::apply`, which also validates it (unknown dataset, coordinates
   outside the grid are ignored). Add a unit test in `session/mod.rs`; no egui
   is involved.
5. **Test it.** Put logic in plain functions and test them without egui (see
   `summary_is_magnitude_and_letter_per_axis`). For looks, add an
   `egui_kittest` snapshot in `src/app.rs` and read the image before you
   commit it (`UPDATE_SNAPSHOTS=1 cargo test`).

That is all: the shelf tile, the card chrome (drag handle, fold, close,
pop-out placeholder), workspaces, the rail and its pop-over come for free.

## A tool with several cards

`Tool::instances(cx)` returns the tool's cards (default: one). Define Overlay
returns one per overlay layer, with a title ("Overlay 2 · stats") and the
layer's id as `Instance::id`; `card_ui` and `summary` receive the `Instance`
and look up their layer with `cx.overlay(LayerId(instance.id))`. The framework
gives each instance its own widget ids, its own fold state, and no × (the
cards are added and removed where the tool manages them). A tool with no
items should still return one placeholder instance so its card says what to
do.

## What a tool gets for free

- **Shelf tile** with open / folded / off states, and a place in every
  workspace (new tools appear *off* in saved workspaces).
- **Card chrome**: drag to reorder, click the title to fold (Alt-click folds
  all), × to hide (the card keeps its settings). `pinned()` replaces × with a
  pin: the card cannot be hidden.
- **Rail**: when the controller is collapsed, the tool's icon opens the same
  card as a pop-over.
- **Persistence**: workspaces and the rail state are saved by eframe.

## Not yet

- `attaches_to()` makes a tool **hooked**: its cards are drawn under the parent's card with the same instance id (for Overlay, the layer number), linked by a spine whose socket shows `link_label()`. A hooked tool also implements `is_hooked(cx, parent)` and `hook_action(parent, on)` (what the parent's attach chip does) and may implement `opened(cx)` (what its tile does when turned on). Clusterize (`tools/clusterize/`) is the example; `session/graph.rs` reads the rules.
- `paint_view()` (drawing on the slice views, e.g. the InstaCorr seed) is
  Milestone 9.
- Pop-out into its own window is Milestone 10.
