//! The tool dependency graph ("hooks"): which tool attaches under which, and
//! the order to recompute downstream results when something upstream changes.
//!
//! The rules live in code, one line per tool: [`Tool::attaches_to`]
//! (`crate::tools::Tool`). This module only reads them. AFNI's dependencies
//! are few and fixed (Clusterize reads the overlay's threshold; later an
//! InstaCorr seed feeds an overlay), so they form short linear chains, not a
//! free-form node graph.

use crate::tools::ToolId;

/// The tool `tool` hooks under, if any.
pub fn parent(tool: ToolId) -> Option<ToolId> {
    tool.tool().and_then(|t| t.attaches_to())
}

/// The tools that hook directly under `tool`, in shelf order.
pub fn children(tool: ToolId) -> Vec<ToolId> {
    ToolId::SHELF
        .into_iter()
        .filter(|t| parent(*t) == Some(tool))
        .collect()
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "InstaCorr feeding an overlay feeding clusters (Milestone 9)"
    )
)]
/// Everything downstream of `tool`, nearest first: the order to recompute
/// when `tool`'s result changes.
pub fn downstream(tool: ToolId) -> Vec<ToolId> {
    let mut out = Vec::new();
    let mut frontier = vec![tool];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for t in frontier {
            for c in children(t) {
                if !out.contains(&c) {
                    out.push(c);
                    next.push(c);
                }
            }
        }
        frontier = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusterize_hooks_under_overlay() {
        assert_eq!(parent(ToolId::Clusterize), Some(ToolId::Overlay));
        assert_eq!(parent(ToolId::Overlay), None);
        assert_eq!(children(ToolId::Overlay), [ToolId::Clusterize]);
        assert!(children(ToolId::Crosshair).is_empty());
    }

    #[test]
    fn downstream_lists_the_chain_nearest_first() {
        assert_eq!(downstream(ToolId::Overlay), [ToolId::Clusterize]);
        assert!(downstream(ToolId::Clusterize).is_empty());
        // Tools not built yet have no rules and no children.
        assert!(parent(ToolId::InstaCorr).is_none());
    }
}
