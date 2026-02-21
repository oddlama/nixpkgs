use ratatui::style::Color;
use serde_json::Value;

use crate::theme::*;
use super::types::ConfigNode;

// ---------------------------------------------------------------------------
// Nerd font icons & their colors
// ---------------------------------------------------------------------------

/// Folder icon for branches (nf-fa-folder U+F07B)
pub(super) const ICON_BRANCH: &str = "\u{f07b}";
pub(super) const ICON_BRANCH_COLOR: Color = YELLOW;

/// Leaf icons by value type
pub(super) const ICON_BOOL: &str = "\u{f205}";      // nf-fa-toggle_on
pub(super) const ICON_BOOL_COLOR: Color = CYAN;
pub(super) const ICON_STRING: &str = "\u{f10d}";    // nf-fa-quote_left
pub(super) const ICON_STRING_COLOR: Color = GREEN;
pub(super) const ICON_NUMBER: &str = "\u{f292}";    // nf-fa-hashtag
pub(super) const ICON_NUMBER_COLOR: Color = MAGENTA;
pub(super) const ICON_NULL: &str = "\u{f071}";      // nf-fa-exclamation_triangle
pub(super) const ICON_NULL_COLOR: Color = COMMENT;
pub(super) const ICON_ARRAY: &str = "\u{f03a}";     // nf-fa-list
pub(super) const ICON_ARRAY_COLOR: Color = YELLOW;
pub(super) const ICON_OBJECT: &str = "\u{f1b2}";    // nf-fa-cube
pub(super) const ICON_OBJECT_COLOR: Color = COMMENT;
pub(super) const ICON_PHANTOM: &str = "\u{f06a}";   // nf-fa-exclamation_circle
pub(super) const ICON_PHANTOM_COLOR: Color = YELLOW;

pub(super) fn node_icon(node: &ConfigNode) -> (&'static str, Color) {
    match node {
        ConfigNode::Branch(_) => (ICON_BRANCH, ICON_BRANCH_COLOR),
        ConfigNode::Phantom => (ICON_PHANTOM, ICON_PHANTOM_COLOR),
        ConfigNode::Leaf(val) => match val {
            Value::Bool(_) => (ICON_BOOL, ICON_BOOL_COLOR),
            Value::String(_) => (ICON_STRING, ICON_STRING_COLOR),
            Value::Number(_) => (ICON_NUMBER, ICON_NUMBER_COLOR),
            Value::Null => (ICON_NULL, ICON_NULL_COLOR),
            Value::Array(_) => (ICON_ARRAY, ICON_ARRAY_COLOR),
            Value::Object(_) => (ICON_OBJECT, ICON_OBJECT_COLOR),
        },
    }
}

// ---------------------------------------------------------------------------
// Type-based colors
// ---------------------------------------------------------------------------

pub(super) fn node_name_color(node: &ConfigNode) -> Color {
    match node {
        ConfigNode::Branch(_) => BLUE,
        ConfigNode::Phantom => YELLOW,
        ConfigNode::Leaf(val) => match val {
            Value::Bool(_) => CYAN,
            Value::String(_) => GREEN,
            Value::Number(_) => MAGENTA,
            Value::Null => COMMENT,
            Value::Array(_) => YELLOW,
            Value::Object(_) => COMMENT,
        },
    }
}

pub(super) fn value_color(value: &Value) -> Color {
    match value {
        Value::Bool(true) => GREEN,
        Value::Bool(false) => RED,
        Value::Number(_) => MAGENTA,
        Value::String(_) => GREEN,
        Value::Null => COMMENT,
        Value::Array(_) => YELLOW,
        Value::Object(_) => COMMENT,
    }
}
