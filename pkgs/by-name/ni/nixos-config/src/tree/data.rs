use std::collections::HashMap;

use serde_json::Value;

use super::types::{ConfigNode, DepsIndex, DiffContext, DiffFilter, DiffTag, Focus, MillerState};

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

// ---------------------------------------------------------------------------
// Diff helpers
// ---------------------------------------------------------------------------

/// Convert a JSON value into root children (extracted from run()).
pub(super) fn build_root_children(json: &Value) -> Vec<(String, ConfigNode)> {
    match json {
        Value::Object(map) => {
            let mut children: Vec<(String, ConfigNode)> = map
                .iter()
                .map(|(k, v)| (k.clone(), build_config_tree(v)))
                .collect();
            sort_branches_first(&mut children);
            children
        }
        _ => vec![("config".to_string(), build_config_tree(json))],
    }
}

/// Build a union tree from two sorted child lists.
pub(super) fn build_union_tree(
    old_root: &[(String, ConfigNode)],
    new_root: &[(String, ConfigNode)],
) -> Vec<(String, ConfigNode)> {
    let mut result = Vec::new();

    // Collect all unique names
    let mut all_names: Vec<String> = Vec::new();
    for (name, _) in old_root.iter().chain(new_root.iter()) {
        if !all_names.contains(name) {
            all_names.push(name.clone());
        }
    }

    for name in &all_names {
        let old_entry = old_root.iter().find(|(n, _)| n == name);
        let new_entry = new_root.iter().find(|(n, _)| n == name);

        match (old_entry, new_entry) {
            (Some((_, old_node)), Some((_, new_node))) => {
                match (old_node, new_node) {
                    (ConfigNode::Branch(old_ch), ConfigNode::Branch(new_ch)) => {
                        let union_ch = build_union_tree(old_ch, new_ch);
                        result.push((name.clone(), ConfigNode::Branch(union_ch)));
                    }
                    (_, ConfigNode::Leaf(val)) => {
                        // Use NEW value; old stored in DiffContext
                        result.push((name.clone(), ConfigNode::Leaf(val.clone())));
                    }
                    (_, ConfigNode::Branch(_)) => {
                        result.push((name.clone(), new_node.clone_node()));
                    }
                    (_, ConfigNode::Phantom) => {
                        result.push((name.clone(), old_node.clone_node()));
                    }
                }
            }
            (Some((_, node)), None) => {
                result.push((name.clone(), node.clone_node()));
            }
            (None, Some((_, node))) => {
                result.push((name.clone(), node.clone_node()));
            }
            (None, None) => unreachable!(),
        }
    }

    sort_branches_first(&mut result);
    result
}

/// Build diff context by comparing old and new trees.
pub(super) fn build_diff_context(
    old_root: &[(String, ConfigNode)],
    new_root: &[(String, ConfigNode)],
    old_deps: DepsIndex,
    new_deps: DepsIndex,
) -> DiffContext {
    let mut tags = HashMap::new();
    let mut old_values = HashMap::new();
    let mut new_values = HashMap::new();

    diff_walk(old_root, new_root, &[], &mut tags, &mut old_values, &mut new_values, &old_deps, &new_deps);

    // Build value-only tags (ignoring dep-only changes)
    let mut value_tags = HashMap::new();
    value_diff_walk(old_root, new_root, &[], &mut value_tags);

    DiffContext {
        tags,
        value_tags,
        old_values,
        new_values,
        old_deps,
        new_deps,
        filter: DiffFilter::ValueChanged,
    }
}

/// Walk computing tags that only consider value changes (not deps).
fn value_diff_walk(
    old_children: &[(String, ConfigNode)],
    new_children: &[(String, ConfigNode)],
    prefix: &[String],
    tags: &mut HashMap<String, DiffTag>,
) {
    let mut all_names: Vec<String> = Vec::new();
    for (name, _) in old_children.iter().chain(new_children.iter()) {
        if !all_names.contains(name) {
            all_names.push(name.clone());
        }
    }

    for name in &all_names {
        let mut path = prefix.to_vec();
        path.push(name.clone());
        let dot_path = path.join(".");

        let old_entry = old_children.iter().find(|(n, _)| n == name);
        let new_entry = new_children.iter().find(|(n, _)| n == name);

        match (old_entry, new_entry) {
            (None, Some((_, new_node))) => {
                tags.insert(dot_path.clone(), DiffTag::Added);
                tag_all_descendants_simple(new_node, &path, DiffTag::Added, tags);
            }
            (Some((_, old_node)), None) => {
                tags.insert(dot_path.clone(), DiffTag::Removed);
                tag_all_descendants_simple(old_node, &path, DiffTag::Removed, tags);
            }
            (Some((_, old_node)), Some((_, new_node))) => {
                match (old_node, new_node) {
                    (ConfigNode::Branch(old_ch), ConfigNode::Branch(new_ch)) => {
                        value_diff_walk(old_ch, new_ch, &path, tags);
                        let has_changes = tags.iter().any(|(k, v)| {
                            k.starts_with(&dot_path) && k != &dot_path && *v != DiffTag::Unchanged
                        });
                        tags.insert(dot_path, if has_changes { DiffTag::Modified } else { DiffTag::Unchanged });
                    }
                    (ConfigNode::Leaf(old_val), ConfigNode::Leaf(new_val)) => {
                        // Only value equality matters here, NOT deps
                        if old_val == new_val {
                            tags.insert(dot_path, DiffTag::Unchanged);
                        } else {
                            tags.insert(dot_path, DiffTag::Modified);
                        }
                    }
                    (ConfigNode::Phantom, ConfigNode::Phantom) => {
                        // Phantoms have no value, so they're unchanged in value-only mode
                        tags.insert(dot_path, DiffTag::Unchanged);
                    }
                    _ => {
                        // Mixed types = Modified
                        tags.insert(dot_path, DiffTag::Modified);
                    }
                }
            }
            (None, None) => unreachable!(),
        }
    }
}

fn tag_all_descendants_simple(
    node: &ConfigNode,
    path: &[String],
    tag: DiffTag,
    tags: &mut HashMap<String, DiffTag>,
) {
    if let ConfigNode::Branch(children) = node {
        for (name, child) in children {
            let mut child_path = path.to_vec();
            child_path.push(name.clone());
            let dot_path = child_path.join(".");
            tags.insert(dot_path, tag);
            tag_all_descendants_simple(child, &child_path, tag, tags);
        }
    }
}

fn diff_walk(
    old_children: &[(String, ConfigNode)],
    new_children: &[(String, ConfigNode)],
    prefix: &[String],
    tags: &mut HashMap<String, DiffTag>,
    old_values: &mut HashMap<String, Value>,
    new_values: &mut HashMap<String, Value>,
    old_deps: &DepsIndex,
    new_deps: &DepsIndex,
) {
    let mut all_names: Vec<String> = Vec::new();
    for (name, _) in old_children.iter().chain(new_children.iter()) {
        if !all_names.contains(name) {
            all_names.push(name.clone());
        }
    }

    for name in &all_names {
        let mut path = prefix.to_vec();
        path.push(name.clone());
        let dot_path = path.join(".");

        let old_entry = old_children.iter().find(|(n, _)| n == name);
        let new_entry = new_children.iter().find(|(n, _)| n == name);

        match (old_entry, new_entry) {
            (None, Some((_, new_node))) => {
                tags.insert(dot_path.clone(), DiffTag::Added);
                if let ConfigNode::Leaf(val) = new_node {
                    new_values.insert(dot_path.clone(), val.clone());
                }
                // Tag all descendants as Added too
                tag_all_descendants(new_node, &path, DiffTag::Added, tags, &mut HashMap::new(), new_values);
            }
            (Some((_, old_node)), None) => {
                tags.insert(dot_path.clone(), DiffTag::Removed);
                if let ConfigNode::Leaf(val) = old_node {
                    old_values.insert(dot_path.clone(), val.clone());
                }
                // Tag all descendants as Removed too
                tag_all_descendants(old_node, &path, DiffTag::Removed, tags, old_values, &mut HashMap::new());
            }
            (Some((_, old_node)), Some((_, new_node))) => {
                match (old_node, new_node) {
                    (ConfigNode::Branch(old_ch), ConfigNode::Branch(new_ch)) => {
                        diff_walk(old_ch, new_ch, &path, tags, old_values, new_values, old_deps, new_deps);
                        // Branch tag: Modified if any descendant is non-Unchanged
                        let has_changes = tags.iter().any(|(k, v)| {
                            k.starts_with(&dot_path) && k != &dot_path && *v != DiffTag::Unchanged
                        });
                        tags.insert(dot_path, if has_changes { DiffTag::Modified } else { DiffTag::Unchanged });
                    }
                    (ConfigNode::Leaf(old_val), ConfigNode::Leaf(new_val)) => {
                        old_values.insert(dot_path.clone(), old_val.clone());
                        new_values.insert(dot_path.clone(), new_val.clone());
                        let vals_equal = old_val == new_val;
                        let deps_equal = deps_match(old_deps, new_deps, &dot_path);
                        if vals_equal && deps_equal {
                            tags.insert(dot_path, DiffTag::Unchanged);
                        } else {
                            tags.insert(dot_path, DiffTag::Modified);
                        }
                    }
                    (ConfigNode::Phantom, ConfigNode::Phantom) => {
                        let deps_equal = deps_match(old_deps, new_deps, &dot_path);
                        if deps_equal {
                            tags.insert(dot_path, DiffTag::Unchanged);
                        } else {
                            tags.insert(dot_path, DiffTag::Modified);
                        }
                    }
                    _ => {
                        // Mixed types (Leaf/Branch, etc.) = Modified
                        if let ConfigNode::Leaf(val) = old_node {
                            old_values.insert(dot_path.clone(), val.clone());
                        }
                        if let ConfigNode::Leaf(val) = new_node {
                            new_values.insert(dot_path.clone(), val.clone());
                        }
                        tags.insert(dot_path, DiffTag::Modified);
                    }
                }
            }
            (None, None) => unreachable!(),
        }
    }
}

fn tag_all_descendants(
    node: &ConfigNode,
    path: &[String],
    tag: DiffTag,
    tags: &mut HashMap<String, DiffTag>,
    left_values: &mut HashMap<String, Value>,
    right_values: &mut HashMap<String, Value>,
) {
    if let ConfigNode::Branch(children) = node {
        for (name, child) in children {
            let mut child_path = path.to_vec();
            child_path.push(name.clone());
            let dot_path = child_path.join(".");
            tags.insert(dot_path.clone(), tag);
            if let ConfigNode::Leaf(val) = child {
                if tag == DiffTag::Added {
                    right_values.insert(dot_path, val.clone());
                } else {
                    left_values.insert(dot_path, val.clone());
                }
            }
            tag_all_descendants(child, &child_path, tag, tags, left_values, right_values);
        }
    }
}

fn deps_match(old_deps: &DepsIndex, new_deps: &DepsIndex, path: &str) -> bool {
    let old_d = old_deps.dependencies.get(path);
    let new_d = new_deps.dependencies.get(path);
    let old_r = old_deps.dependents.get(path);
    let new_r = new_deps.dependents.get(path);
    old_d == new_d && old_r == new_r
}

/// Filter out unchanged nodes, keeping only changed nodes and their ancestors.
/// Accepts a tag map so it can be called with either `tags` or `value_tags`.
pub(super) fn filter_unchanged_tree(
    root: &[(String, ConfigNode)],
    tags: &HashMap<String, DiffTag>,
    prefix: &[String],
) -> Vec<(String, ConfigNode)> {
    let mut result = Vec::new();
    for (name, node) in root {
        let mut path = prefix.to_vec();
        path.push(name.clone());
        let dot_path = path.join(".");
        let tag = tags.get(&dot_path).copied().unwrap_or(DiffTag::Unchanged);

        match node {
            ConfigNode::Branch(children) => {
                let filtered = filter_unchanged_tree(children, tags, &path);
                if !filtered.is_empty() {
                    result.push((name.clone(), ConfigNode::Branch(filtered)));
                }
            }
            _ => {
                if tag != DiffTag::Unchanged {
                    result.push((name.clone(), node.clone_node()));
                }
            }
        }
    }
    result
}

/// Merge two DepsIndex structs by unioning all edges.
pub(super) fn merge_deps_indices(a: DepsIndex, b: DepsIndex) -> DepsIndex {
    let mut dependencies = a.dependencies;
    let mut dependents = a.dependents;

    for (k, mut v) in b.dependencies {
        dependencies.entry(k).or_default().append(&mut v);
    }
    for (k, mut v) in b.dependents {
        dependents.entry(k).or_default().append(&mut v);
    }

    for v in dependencies.values_mut() {
        v.sort();
        v.dedup();
    }
    for v in dependents.values_mut() {
        v.sort();
        v.dedup();
    }

    DepsIndex { dependencies, dependents }
}

/// Get the effective diff tag for a node, considering descendants for branches.
pub(super) fn get_diff_tag(
    diff_ctx: &DiffContext,
    path_prefix: &[String],
    name: &str,
    node: &ConfigNode,
) -> DiffTag {
    let mut full_path = path_prefix.to_vec();
    full_path.push(name.to_string());
    let dot_path = full_path.join(".");

    let tag = diff_ctx.tags.get(&dot_path).copied().unwrap_or(DiffTag::Unchanged);

    match node {
        ConfigNode::Branch(_) => {
            // For branches: if any descendant is changed, show as Modified
            if tag == DiffTag::Unchanged {
                let prefix_dot = format!("{}.", dot_path);
                let has_changes = diff_ctx.tags.iter().any(|(k, v)| {
                    k.starts_with(&prefix_dot) && *v != DiffTag::Unchanged
                });
                if has_changes { DiffTag::Modified } else { DiffTag::Unchanged }
            } else {
                tag
            }
        }
        _ => tag,
    }
}
