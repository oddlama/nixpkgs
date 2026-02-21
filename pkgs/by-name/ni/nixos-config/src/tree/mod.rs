mod types;
mod icons;
mod data;
mod widgets;
mod render;
mod input;

use std::collections::HashMap;

use anyhow::Result;
use serde_json::Value;

use crate::resolve;
use crate::tui;

use types::*;
use data::*;
use widgets::print_tree_text;
use input::InputAction;

pub fn run(config: &str, explicit: bool, use_color: bool, nix_args: &[String]) -> Result<()> {
    if !use_color {
        let json = resolve::resolve(config, explicit, nix_args)?;
        let root_children: Vec<(String, ConfigNode)> = match &json {
            Value::Object(map) => {
                let mut children: Vec<(String, ConfigNode)> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), build_config_tree(v)))
                    .collect();
                sort_branches_first(&mut children);
                children
            }
            _ => vec![("config".to_string(), build_config_tree(&json))],
        };
        print_tree_text(&root_children, 0);
        return Ok(());
    }

    let combined = resolve::resolve_combined(config, explicit, nix_args)?;
    let json = combined.config_values;

    let mut root_children: Vec<(String, ConfigNode)> = match &json {
        Value::Object(map) => {
            let mut children: Vec<(String, ConfigNode)> = map
                .iter()
                .map(|(k, v)| (k.clone(), build_config_tree(v)))
                .collect();
            sort_branches_first(&mut children);
            children
        }
        _ => vec![("config".to_string(), build_config_tree(&json))],
    };

    let deps_index = build_deps_index(&combined.filtered_deps);
    insert_phantom_nodes(&mut root_children, &deps_index);

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
        let middle_children = get_children_at_path(&root_children, &state.path);
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
                &root_children,
                &deps_index,
                config,
                &status_msg,
                &mut pane_areas,
            );
        })?;

        let input = tui::read_input()?;
        let terminal_height = terminal.size()?.height;

        match input::handle_input(
            &input,
            &mut mode,
            &mut state,
            &root_children,
            &deps_index,
            &mut status_msg,
            &pane_areas,
            terminal_height,
            middle_count,
        )? {
            InputAction::Quit => break,
            InputAction::Continue => {}
        }
    }

    tui::teardown(terminal)?;
    Ok(())
}
