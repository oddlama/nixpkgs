use std::collections::HashMap;

use serde_json::Value;

use super::types::{ConfigNode, DepsIndex, Focus, MillerState};

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

pub(super) fn sort_branches_first(children: &mut [(String, ConfigNode)]) {
    children.sort_by(|(a, na), (b, nb)| {
        let a_branch = matches!(na, ConfigNode::Branch(_));
        let b_branch = matches!(nb, ConfigNode::Branch(_));
        b_branch.cmp(&a_branch).then_with(|| a.cmp(b))
    });
}

pub(super) fn build_config_tree(value: &Value) -> ConfigNode {
    match value {
        Value::Object(map) if map.is_empty() => ConfigNode::Leaf(value.clone()),
        Value::Object(map) => {
            let mut children: Vec<(String, ConfigNode)> = map
                .iter()
                .map(|(k, v)| (k.clone(), build_config_tree(v)))
                .collect();
            sort_branches_first(&mut children);
            ConfigNode::Branch(children)
        }
        _ => ConfigNode::Leaf(value.clone()),
    }
}

pub(super) fn get_children_at_path<'a>(
    root: &'a [(String, ConfigNode)],
    path: &[String],
) -> Option<&'a [(String, ConfigNode)]> {
    let mut current = root;
    for segment in path {
        let idx = current.iter().position(|(n, _)| n == segment)?;
        match &current[idx].1 {
            ConfigNode::Branch(children) => current = children,
            ConfigNode::Leaf(_) | ConfigNode::Phantom => return None,
        }
    }
    Some(current)
}

pub(super) fn get_node_at_path<'a>(
    root: &'a [(String, ConfigNode)],
    path: &[String],
    name: &str,
) -> Option<&'a ConfigNode> {
    let children = get_children_at_path(root, path)?;
    children.iter().find(|(n, _)| n == name).map(|(_, node)| node)
}

pub(super) fn lookup_node<'a>(root: &'a [(String, ConfigNode)], path: &[String]) -> Option<&'a ConfigNode> {
    if path.is_empty() {
        return None;
    }
    get_node_at_path(root, &path[..path.len() - 1], &path[path.len() - 1])
}

// ---------------------------------------------------------------------------
// Dependency index
// ---------------------------------------------------------------------------

pub(super) fn build_deps_index(deps_json: &Value) -> DepsIndex {
    let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

    if let Value::Array(entries) = deps_json {
        for entry in entries {
            let accessor = entry
                .get("accessor")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                });
            let accessed = entry
                .get("accessed")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                });

            if let (Some(accessor), Some(accessed)) = (accessor, accessed) {
                dependencies
                    .entry(accessor.clone())
                    .or_default()
                    .push(accessed.clone());
                dependents.entry(accessed).or_default().push(accessor);
            }
        }
    }

    for v in dependencies.values_mut() {
        v.sort();
        v.dedup();
    }
    for v in dependents.values_mut() {
        v.sort();
        v.dedup();
    }

    DepsIndex {
        dependencies,
        dependents,
    }
}

/// Insert placeholder nodes for dependency paths not present in the config tree.
pub(super) fn insert_phantom_nodes(
    root: &mut Vec<(String, ConfigNode)>,
    deps_index: &DepsIndex,
) {
    use std::collections::HashSet;

    let mut all_paths: HashSet<&str> = HashSet::new();
    for key in deps_index.dependencies.keys() {
        all_paths.insert(key.as_str());
    }
    for key in deps_index.dependents.keys() {
        all_paths.insert(key.as_str());
    }
    for vals in deps_index.dependencies.values() {
        for v in vals {
            all_paths.insert(v.as_str());
        }
    }
    for vals in deps_index.dependents.values() {
        for v in vals {
            all_paths.insert(v.as_str());
        }
    }

    for dot_path in all_paths {
        let segments: Vec<&str> = dot_path.split('.').collect();
        ensure_path_exists(root, &segments);
    }

    sort_tree_recursive(root);
}

fn ensure_path_exists(root: &mut Vec<(String, ConfigNode)>, segments: &[&str]) {
    if segments.is_empty() {
        return;
    }

    let name = segments[0].to_string();
    let rest = &segments[1..];

    let pos = root.iter().position(|(n, _)| *n == name);
    let idx = if let Some(i) = pos {
        i
    } else {
        if rest.is_empty() {
            root.push((name, ConfigNode::Phantom));
        } else {
            root.push((name, ConfigNode::Branch(Vec::new())));
        }
        root.len() - 1
    };

    if rest.is_empty() {
        return;
    }

    match &mut root[idx].1 {
        ConfigNode::Branch(children) => {
            ensure_path_exists(children, rest);
        }
        ConfigNode::Leaf(_) | ConfigNode::Phantom => {
            let mut children = Vec::new();
            ensure_path_exists(&mut children, rest);
            root[idx].1 = ConfigNode::Branch(children);
        }
    }
}

fn sort_tree_recursive(children: &mut [(String, ConfigNode)]) {
    sort_branches_first(children);
    for (_, node) in children.iter_mut() {
        if let ConfigNode::Branch(ch) = node {
            sort_tree_recursive(ch);
        }
    }
}

// ---------------------------------------------------------------------------
// Value formatting
// ---------------------------------------------------------------------------

pub(super) fn format_value_short(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.contains('\n') {
                "''...''".to_string()
            } else if s.len() > 40 {
                format!("\"{}...\"", &s[..37])
            } else {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
        }
        Value::Array(arr) if arr.is_empty() => "[]".to_string(),
        Value::Array(arr) => format!("[{}]", arr.len()),
        Value::Object(map) if map.is_empty() => "{}".to_string(),
        Value::Object(map) => format!("{{{}}}", map.len()),
    }
}

pub(super) fn format_value_full(value: &Value) -> Vec<String> {
    match value {
        Value::Null => vec!["null".to_string()],
        Value::Bool(b) => vec![b.to_string()],
        Value::Number(n) => vec![n.to_string()],
        Value::String(s) => {
            if s.contains('\n') {
                let mut lines = vec!["''".to_string()];
                for line in s.lines() {
                    lines.push(format!("  {}", line));
                }
                lines.push("''".to_string());
                lines
            } else {
                vec![format!(
                    "\"{}\"",
                    s.replace('\\', "\\\\").replace('"', "\\\"")
                )]
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                return vec!["[ ]".to_string()];
            }
            let mut lines = vec!["[".to_string()];
            for item in arr {
                lines.push(format!("  {}", format_value_short(item)));
            }
            lines.push("]".to_string());
            lines
        }
        Value::Object(map) => {
            if map.is_empty() {
                return vec!["{ }".to_string()];
            }
            let mut lines = vec!["{".to_string()];
            for (k, v) in map {
                lines.push(format!("  {} = {};", k, format_value_short(v)));
            }
            lines.push("}".to_string());
            lines
        }
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

pub(super) fn search_tree(
    root: &[(String, ConfigNode)],
    query: &str,
    prefix: &[String],
) -> Vec<Vec<String>> {
    let mut results = Vec::new();
    if query.is_empty() {
        return results;
    }
    let lower_query = query.to_lowercase();
    for (name, node) in root {
        let mut full_path = prefix.to_vec();
        full_path.push(name.clone());
        let joined = full_path.join(".");
        if joined.to_lowercase().contains(&lower_query) {
            results.push(full_path.clone());
        }
        if let ConfigNode::Branch(children) = node {
            results.extend(search_tree(children, query, &full_path));
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Navigation helpers
// ---------------------------------------------------------------------------

pub(super) fn jump_to_path(state: &mut MillerState, target_path: &[String], root: &[(String, ConfigNode)]) {
    if target_path.is_empty() {
        return;
    }

    state
        .path_memory
        .insert(state.path.clone(), (state.cursor, state.scroll));

    state.path.clear();
    state.scroll = 0;
    state.detail_scroll = 0;
    state.deps_cursor = 0;
    state.deps_scroll = 0;

    let parent = &target_path[..target_path.len() - 1];
    let leaf = &target_path[target_path.len() - 1];

    let mut current = root;
    for segment in parent {
        if let Some(idx) = current.iter().position(|(n, _)| n == segment) {
            state.path_memory.insert(state.path.clone(), (idx, 0));
            state.path.push(segment.clone());
            match &current[idx].1 {
                ConfigNode::Branch(children) => current = children.as_slice(),
                _ => return,
            }
        } else {
            return;
        }
    }

    state.cursor = current.iter().position(|(n, _)| n == leaf).unwrap_or(0);
    state.focus = Focus::Middle;
}

pub(super) fn clamp_cursor(cursor: &mut usize, scroll: &mut usize, total: usize, visible: usize) {
    if total == 0 {
        *cursor = 0;
        *scroll = 0;
        return;
    }
    if *cursor >= total {
        *cursor = total - 1;
    }
    if *cursor < *scroll {
        *scroll = *cursor;
    } else if *cursor >= *scroll + visible {
        *scroll = *cursor - visible + 1;
    }
}

pub(super) fn current_full_path(state: &MillerState, root: &[(String, ConfigNode)]) -> Vec<String> {
    let mut p = state.path.clone();
    if let Some(children) = get_children_at_path(root, &state.path) {
        if let Some((name, _)) = children.get(state.cursor) {
            p.push(name.clone());
        }
    }
    p
}
