mod types;
mod icons;
mod data;
mod widgets;
mod render;
mod input;

use std::collections::HashMap;

use anyhow::Result;

use crate::resolve;
use crate::tui;

use types::*;
use data::*;
use widgets::print_tree_text;
use input::InputAction;

pub fn run(config: &str, explicit: bool, use_color: bool, nix_args: &[String]) -> Result<()> {
    if !use_color {
        let json = resolve::resolve(config, explicit, nix_args)?;
        let root_children = build_root_children(&json);
        print_tree_text(&root_children, 0);
        return Ok(());
    }

    let combined = resolve::resolve_combined(config, explicit, nix_args)?;
    let json = combined.config_values;
    let mut root_children = build_root_children(&json);

    let deps_index = build_deps_index(&combined.filtered_deps);
    insert_phantom_nodes(&mut root_children, &deps_index);

    run_inner(&root_children, &root_children, &root_children, &deps_index, config, None)
}

pub fn run_diff(
    old_arg: &str,
    new_arg: &str,
    explicit: bool,
    use_color: bool,
    nix_args: &[String],
) -> Result<()> {
    if !use_color {
        eprintln!("diff requires a terminal with color support");
        return Ok(());
    }

    let old_combined = resolve::resolve_combined(old_arg, explicit, nix_args)?;
    let new_combined = resolve::resolve_combined(new_arg, explicit, nix_args)?;

    let old_root = build_root_children(&old_combined.config_values);
    let new_root = build_root_children(&new_combined.config_values);

    let old_deps = build_deps_index(&old_combined.filtered_deps);
    let new_deps = build_deps_index(&new_combined.filtered_deps);

    // Merge deps for phantom nodes
    let merged_deps = merge_deps_indices(
        build_deps_index(&old_combined.filtered_deps),
        build_deps_index(&new_combined.filtered_deps),
    );

    // Build union tree
    let mut union_tree = build_union_tree(&old_root, &new_root);
    insert_phantom_nodes(&mut union_tree, &merged_deps);

    // Build diff context
    let diff_ctx = build_diff_context(&old_root, &new_root, old_deps, new_deps);

    // Build filtered trees (only changed nodes)
    let filtered_tree = filter_unchanged_tree(&union_tree, &diff_ctx.tags, &[]);
    let value_filtered_tree = filter_unchanged_tree(&union_tree, &diff_ctx.value_tags, &[]);

    let label = format!("{} -> {}", old_arg, new_arg);
    run_inner(&union_tree, &filtered_tree, &value_filtered_tree, &merged_deps, &label, Some(diff_ctx))
}

fn run_inner(
    full_root: &[(String, ConfigNode)],
    filtered_root: &[(String, ConfigNode)],
    value_filtered_root: &[(String, ConfigNode)],
    deps_index: &DepsIndex,
    config: &str,
    mut diff_ctx: Option<DiffContext>,
) -> Result<()> {
    let mut state = MillerState {
        path: Vec::new(),
        cursor: 0,
        scroll: 0,
        path_memory: HashMap::new(),
        detail_scroll: 0,
        deps_cursor: 0,
        deps_scroll: 0,
        focus: Focus::Middle,
    };

    let mut mode = Mode::Normal;
    let mut status_msg: Option<String> = None;
    let mut pane_areas = PaneAreas::default();

    let mut terminal = tui::setup()?;

    loop {
        let active_root = match diff_ctx.as_ref().map(|c| c.filter) {
            Some(DiffFilter::Changed) => filtered_root,
            Some(DiffFilter::ValueChanged) => value_filtered_root,
            _ => full_root,
        };

        let middle_children = get_children_at_path(active_root, &state.path);
        let middle_count = middle_children.map(|c| c.len()).unwrap_or(0);

        if middle_count == 0 {
            state.cursor = 0;
        } else if state.cursor >= middle_count {
            state.cursor = middle_count - 1;
        }

        terminal.draw(|frame| {
            render::render_frame(
                frame,
                &mode,
                &mut state,
                active_root,
                deps_index,
                config,
                &status_msg,
                &mut pane_areas,
                diff_ctx.as_ref(),
            );
        })?;

        let input = tui::read_input()?;
        let terminal_height = terminal.size()?.height;

        match input::handle_input(
            &input,
            &mut mode,
            &mut state,
            active_root,
            deps_index,
            &mut status_msg,
            &pane_areas,
            terminal_height,
            middle_count,
            diff_ctx.as_ref(),
        )? {
            InputAction::Quit => break,
            InputAction::Continue => {}
            InputAction::ToggleUnchanged => {
                if let Some(ref mut ctx) = diff_ctx {
                    ctx.filter = ctx.filter.next();
                    // Reset navigation
                    state.path.clear();
                    state.cursor = 0;
                    state.scroll = 0;
                    state.detail_scroll = 0;
                    state.deps_cursor = 0;
                    state.deps_scroll = 0;
                    state.path_memory.clear();
                }
            }
        }
    }

    tui::teardown(terminal)?;
    Ok(())
}
