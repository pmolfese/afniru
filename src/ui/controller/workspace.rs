//! Workspaces: named tool sets with card order and fold state.
//!
//! Pure data (no egui), saved with the app. The *current* workspace is edited
//! live: turning a tile on, folding a card or dragging one to a new place
//! changes it, so it is exactly as you left it next time. "Save as" copies it
//! under a new name.

use serde::{Deserialize, Serialize};

use crate::tools::ToolId;

/// One tool's place in a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardEntry {
    /// Which tool.
    pub tool: ToolId,
    /// Card shown in the stack? An off tool keeps its place and fold state.
    pub on: bool,
    /// Folded to its summary line?
    pub collapsed: bool,
}

/// A named arrangement of cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    /// The name in the workspace menu.
    pub name: String,
    /// Every tool, in card order (off ones included).
    pub cards: Vec<CardEntry>,
}

impl Workspace {
    /// A workspace with Datasets, Overlay and Crosshair open and everything else
    /// off.
    pub fn standard(name: &str) -> Self {
        let cards = ToolId::SHELF
            .iter()
            .map(|&tool| CardEntry {
                tool,
                on: matches!(tool, ToolId::Datasets | ToolId::Overlay | ToolId::Crosshair),
                collapsed: false,
            })
            .collect();
        Self {
            name: name.to_string(),
            cards,
        }
    }

    /// Make the workspace complete and valid: every tool exactly once
    /// (missing ones appended off, duplicates dropped) and pinned tools on.
    /// Applied after loading, so files from older versions keep working when
    /// tools are added.
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.cards.retain(|c| {
            let fresh = !seen.contains(&c.tool);
            seen.push(c.tool);
            fresh
        });
        for tool in ToolId::SHELF {
            if !seen.contains(&tool) {
                self.cards.push(CardEntry {
                    tool,
                    on: false,
                    collapsed: false,
                });
            }
        }
        for c in &mut self.cards {
            if pinned(c.tool) {
                c.on = true;
            }
        }
    }

    fn entry(&self, tool: ToolId) -> Option<&CardEntry> {
        self.cards.iter().find(|c| c.tool == tool)
    }

    fn entry_mut(&mut self, tool: ToolId) -> Option<&mut CardEntry> {
        self.cards.iter_mut().find(|c| c.tool == tool)
    }

    /// The tool's entry state, for the shelf tile.
    pub fn state(&self, tool: ToolId) -> Option<CardEntry> {
        self.entry(tool).copied()
    }

    /// Tools whose cards are shown (on and implemented), in order.
    pub fn visible(&self) -> Vec<ToolId> {
        self.cards
            .iter()
            .filter(|c| c.on && c.tool.tool().is_some())
            .map(|c| c.tool)
            .collect()
    }

    /// Click on a tile: show the card (unfolded), or hide it. Pinned tools
    /// and tools that do not exist yet are left alone.
    pub fn toggle(&mut self, tool: ToolId) {
        if pinned(tool) || tool.tool().is_none() {
            return;
        }
        if let Some(c) = self.entry_mut(tool) {
            c.on = !c.on;
            if c.on {
                c.collapsed = false;
            }
        }
    }

    /// Make sure the tool's card is shown (a tile already on is left as it
    /// is, folded or not). Pinned tools and tools not built yet are ignored.
    pub fn show(&mut self, tool: ToolId) {
        if tool.tool().is_none() {
            return;
        }
        if let Some(c) = self.entry_mut(tool)
            && !c.on
        {
            c.on = true;
            c.collapsed = false;
        }
    }

    /// Hide a card (the × button). Its settings and place are kept.
    pub fn close(&mut self, tool: ToolId) {
        if !pinned(tool)
            && let Some(c) = self.entry_mut(tool)
        {
            c.on = false;
        }
    }

    /// Fold or unfold one card.
    pub fn toggle_collapsed(&mut self, tool: ToolId) {
        if let Some(c) = self.entry_mut(tool) {
            c.collapsed = !c.collapsed;
        }
    }

    /// Fold every shown card, or unfold them all.
    pub fn set_all_collapsed(&mut self, collapsed: bool) {
        for c in &mut self.cards {
            c.collapsed = collapsed;
        }
    }

    /// Move `tool`'s card so it is the `index`th among the *shown* cards
    /// (clamped). Hidden tools keep their relative order.
    pub fn move_card(&mut self, tool: ToolId, index: usize) {
        let Some(from) = self.cards.iter().position(|c| c.tool == tool) else {
            return;
        };
        let entry = self.cards.remove(from);
        let shown: Vec<usize> = (0..self.cards.len())
            .filter(|&i| self.cards[i].on && self.cards[i].tool.tool().is_some())
            .collect();
        let at = match shown.get(index) {
            Some(&i) => i,
            None => shown.last().map_or(self.cards.len(), |&i| i + 1),
        };
        self.cards.insert(at, entry);
    }
}

fn pinned(tool: ToolId) -> bool {
    tool.tool().is_some_and(|t| t.pinned())
}

/// All workspaces and which one is current.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspaces {
    /// The workspaces, in menu order. Never empty.
    pub list: Vec<Workspace>,
    /// Index of the current one.
    pub current: usize,
}

impl Default for Workspaces {
    fn default() -> Self {
        Self {
            list: vec![Workspace::standard("Default")],
            current: 0,
        }
    }
}

impl Workspaces {
    /// Repair after loading: valid index, at least one workspace, every
    /// workspace complete.
    pub fn normalize(&mut self) {
        if self.list.is_empty() {
            *self = Self::default();
        }
        self.list.iter_mut().for_each(Workspace::normalize);
        self.current = self.current.min(self.list.len() - 1);
    }

    /// The current workspace.
    pub fn current(&self) -> &Workspace {
        &self.list[self.current]
    }

    /// The current workspace, mutably (edits are live).
    pub fn current_mut(&mut self) -> &mut Workspace {
        &mut self.list[self.current]
    }

    /// Switch to workspace `index` (ignored if out of range).
    pub fn switch(&mut self, index: usize) {
        if index < self.list.len() {
            self.current = index;
        }
    }

    /// Copy the current workspace under `name` and switch to the copy. A
    /// blank name is ignored; an existing name is replaced.
    pub fn save_as(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let mut copy = self.current().clone();
        copy.name = name.to_string();
        match self.list.iter().position(|w| w.name == name) {
            Some(i) => {
                self.list[i] = copy;
                self.current = i;
            }
            None => {
                self.list.push(copy);
                self.current = self.list.len() - 1;
            }
        }
    }

    /// Delete the current workspace (the last one cannot be deleted).
    pub fn delete_current(&mut self) {
        if self.list.len() > 1 {
            self.list.remove(self.current);
            self.current = self.current.saturating_sub(1).min(self.list.len() - 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(w: &Workspace) -> Vec<ToolId> {
        w.visible()
    }

    #[test]
    fn standard_shows_datasets_overlay_then_crosshair() {
        let w = Workspace::standard("x");
        assert_eq!(
            visible(&w),
            [ToolId::Datasets, ToolId::Overlay, ToolId::Crosshair]
        );
        assert_eq!(w.cards.len(), 10);
    }

    #[test]
    fn toggling_hides_and_shows_and_keeps_fold_state() {
        let mut w = Workspace::standard("x");
        w.toggle_collapsed(ToolId::Crosshair);
        w.toggle(ToolId::Crosshair);
        assert_eq!(visible(&w), [ToolId::Datasets, ToolId::Overlay]);
        assert!(w.state(ToolId::Crosshair).unwrap().collapsed); // kept while off
        w.toggle(ToolId::Crosshair); // turning on unfolds
        assert_eq!(
            visible(&w),
            [ToolId::Datasets, ToolId::Overlay, ToolId::Crosshair]
        );
        assert!(!w.state(ToolId::Crosshair).unwrap().collapsed);
    }

    #[test]
    fn pinned_and_unbuilt_tools_ignore_toggle_and_close() {
        let mut w = Workspace::standard("x");
        w.toggle(ToolId::Datasets);
        w.close(ToolId::Datasets);
        w.toggle(ToolId::InstaCorr); // not built yet
        assert_eq!(
            visible(&w),
            [ToolId::Datasets, ToolId::Overlay, ToolId::Crosshair]
        );
        assert!(!w.state(ToolId::InstaCorr).unwrap().on);
    }

    #[test]
    fn collapse_all_and_expand_all() {
        let mut w = Workspace::standard("x");
        w.set_all_collapsed(true);
        assert!(w.cards.iter().all(|c| c.collapsed));
        w.set_all_collapsed(false);
        assert!(w.cards.iter().all(|c| !c.collapsed));
    }

    #[test]
    fn move_card_reorders_among_shown_cards() {
        let mut w = Workspace::standard("x");
        w.move_card(ToolId::Crosshair, 0);
        assert_eq!(
            visible(&w),
            [ToolId::Crosshair, ToolId::Datasets, ToolId::Overlay]
        );
        w.move_card(ToolId::Crosshair, 99); // clamped to the end
        assert_eq!(
            visible(&w),
            [ToolId::Datasets, ToolId::Overlay, ToolId::Crosshair]
        );
        assert_eq!(w.cards.len(), 10);
    }

    #[test]
    fn normalize_completes_dedups_and_pins() {
        let mut w = Workspace {
            name: "old".into(),
            cards: vec![
                CardEntry {
                    tool: ToolId::Crosshair,
                    on: true,
                    collapsed: true,
                },
                CardEntry {
                    tool: ToolId::Datasets,
                    on: false,
                    collapsed: false,
                },
                CardEntry {
                    tool: ToolId::Crosshair,
                    on: false,
                    collapsed: false,
                },
            ],
        };
        w.normalize();
        assert_eq!(w.cards.len(), 10);
        assert_eq!(w.cards[0].tool, ToolId::Crosshair);
        assert!(w.cards[0].collapsed); // the first duplicate won
        assert!(w.state(ToolId::Datasets).unwrap().on);
        assert!(!w.state(ToolId::Overlay).unwrap().on);
    }

    #[test]
    fn workspaces_save_switch_and_delete() {
        let mut ws = Workspaces::default();
        ws.current_mut().toggle(ToolId::Crosshair);
        ws.save_as("  Lean  ");
        assert_eq!(ws.list.len(), 2);
        assert_eq!(ws.current().name, "Lean");
        assert_eq!(visible(ws.current()), [ToolId::Datasets, ToolId::Overlay]);
        // The original is untouched by later edits to the copy.
        ws.current_mut().toggle(ToolId::Crosshair);
        ws.switch(0);
        assert_eq!(visible(ws.current()), [ToolId::Datasets, ToolId::Overlay]); // Default was edited before the copy
        ws.save_as("Lean"); // same name replaces
        assert_eq!(ws.list.len(), 2);
        ws.save_as("   "); // blank ignored
        assert_eq!(ws.list.len(), 2);
        ws.delete_current();
        assert_eq!(ws.list.len(), 1);
        ws.delete_current(); // the last cannot go
        assert_eq!(ws.list.len(), 1);
        ws.switch(5);
        assert_eq!(ws.current, 0);
    }

    #[test]
    fn workspaces_normalize_repairs_bad_state() {
        let mut ws = Workspaces {
            list: vec![],
            current: 7,
        };
        ws.normalize();
        assert_eq!(ws.list.len(), 1);
        assert_eq!(ws.current, 0);
    }

    #[test]
    fn round_trips_through_ron() {
        let mut ws = Workspaces::default();
        ws.current_mut().toggle_collapsed(ToolId::Crosshair);
        ws.save_as("Mine");
        let text = ron_string(&ws);
        let back: Workspaces = ron_parse(&text);
        assert_eq!(back, ws);
    }

    // eframe stores values as RON; check the same encoding here.
    fn ron_string(ws: &Workspaces) -> String {
        let mut storage = MemStorage::default();
        eframe::set_value(&mut storage, "k", ws);
        storage.0.remove("k").unwrap()
    }

    fn ron_parse(text: &str) -> Workspaces {
        let mut storage = MemStorage::default();
        storage.0.insert("k".into(), text.into());
        eframe::get_value(&storage, "k").unwrap()
    }

    #[derive(Default)]
    struct MemStorage(std::collections::HashMap<String, String>);

    impl eframe::Storage for MemStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }
}
