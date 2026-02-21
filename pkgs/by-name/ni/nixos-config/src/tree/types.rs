use std::collections::HashMap;

use ratatui::layout::Rect;
use ratatui::style::Color;
use serde_json::Value;

pub(super) enum ConfigNode {
    Branch(Vec<(String, ConfigNode)>),
    Leaf(Value),
    /// A node that appears in the dependency graph but has no serializable value.
    Phantom,
}

pub(super) struct DepsIndex {
    pub dependencies: HashMap<String, Vec<String>>,
    pub dependents: HashMap<String, Vec<String>>,
}

#[derive(Clone, PartialEq)]
pub(super) enum Focus {
    Middle,
    Detail,
    Deps,
    Revs,
}

pub(super) struct MillerState {
    pub path: Vec<String>,
    pub cursor: usize,
    pub scroll: usize,
    pub path_memory: HashMap<Vec<String>, (usize, usize)>,
    pub detail_scroll: usize,
    pub deps_cursor: usize,
    pub deps_scroll: usize,
    pub focus: Focus,
}

pub(super) enum Mode {
    Normal,
    Search {
        query: String,
        results: Vec<Vec<String>>,
        cursor: usize,
        scroll: usize,
        right_focus: Focus,
        detail_scroll: usize,
        deps_cursor: usize,
        deps_scroll: usize,
    },
    Help,
    Pager {
        path: Vec<String>,
        lines: Vec<String>,
        scroll: usize,
        color: Color,
    },
}

pub(super) fn rect_contains(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

/// Remembered inner areas for mouse hit-testing.
#[derive(Default)]
pub(super) struct PaneAreas {
    // Normal mode
    pub parent_inner: Rect,
    pub browse_inner: Rect,
    pub children_inner: Rect,
    pub detail_inner: Rect,
    pub deps_inner: Rect,
    pub revs_inner: Rect,
    // Scroll offsets needed to convert row → item index
    pub browse_scroll: usize,
    pub deps_scroll: usize,
    // Normal mode: counts for bounds-checking
    pub browse_count: usize,
    pub deps_count: usize,
    pub revs_count: usize,
    // Search mode
    pub search_results_inner: Rect,
    pub search_results_scroll: usize,
    pub search_results_count: usize,
    pub search_detail_inner: Rect,
    pub search_deps_inner: Rect,
    pub search_revs_inner: Rect,
    pub search_deps_count: usize,
    pub search_revs_count: usize,
    pub search_deps_scroll: usize,
}
