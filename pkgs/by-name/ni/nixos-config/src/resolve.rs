use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// Resolve a config argument to a parsed JSON value.
///
/// Resolution order:
/// 1. Existing directory → read tracking JSON from it
/// 2. Flake reference (contains `#`) → evaluate via `nix eval`
/// 3. Error
pub fn resolve(arg: &str, explicit: bool, nix_args: &[String]) -> Result<Value> {
    let path = Path::new(arg);
    if path.is_dir() {
        return resolve_dir(path, explicit);
    }
    if arg.contains('#') {
        return resolve_flake(arg, explicit, nix_args);
    }
    bail!(
        "'{}' is not an existing directory or flake reference (flake refs must contain '#')",
        arg
    );
}

/// Resolve a config argument to dependency tracking data (filteredDeps).
#[allow(dead_code)]
pub fn resolve_deps(arg: &str, nix_args: &[String]) -> Result<Value> {
    let path = Path::new(arg);
    if path.is_dir() {
        return resolve_deps_dir(path);
    }
    if arg.contains('#') {
        return resolve_deps_flake(arg, nix_args);
    }
    bail!(
        "'{}' is not an existing directory or flake reference (flake refs must contain '#')",
        arg
    );
}

/// Read tracking-deps.json from a directory.
fn resolve_deps_dir(dir: &Path) -> Result<Value> {
    let file = dir.join("tracking-deps.json");
    let contents = std::fs::read_to_string(&file)
        .with_context(|| format!("reading {}", file.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parsing JSON from {}", file.display()))
}

/// Resolve dependency data from a flake reference via `nix eval`.
fn resolve_deps_flake(reference: &str, nix_args: &[String]) -> Result<Value> {
    if let Some((flake, attr)) = reference.split_once('#') {
        let nixos_config_ref = format!(
            "{}#nixosConfigurations.{}.dependencyTracking.filteredDeps",
            flake, attr
        );
        eprintln!("Evaluating deps for {}#{}...", flake, attr);

        match nix_eval_json(&nixos_config_ref, nix_args) {
            Ok(value) => return Ok(value),
            Err(e) => eprintln!("  tried {}: {:#}", nixos_config_ref, e),
        }

        // Try the reference as-is
        let direct_ref = format!("{}.dependencyTracking.filteredDeps", reference);
        match nix_eval_json(&direct_ref, nix_args) {
            Ok(value) => return Ok(value),
            Err(e) => eprintln!("  tried {}: {:#}", direct_ref, e),
        }

        bail!(
            "Could not evaluate dependency data for '{}'.\n\
             Make sure the flake's nixosSystem is called with trackDependencies = true\n\
             and uses a nixpkgs that supports dependency tracking.",
            reference
        );
    }

    bail!("Invalid flake reference: {}", reference);
}

/// Read tracking JSON from a directory (e.g., a store path or /var/run/current-system).
fn resolve_dir(dir: &Path, explicit: bool) -> Result<Value> {
    let filename = if explicit {
        "tracking-explicit.json"
    } else {
        "tracking.json"
    };
    let file = dir.join(filename);
    let contents = std::fs::read_to_string(&file)
        .with_context(|| format!("reading {}", file.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parsing JSON from {}", file.display()))
}

/// Resolve a flake reference via `nix eval`.
///
/// Tries several attribute paths in order:
/// 1. `<flake>#nixosConfigurations.<attr>.dependencyTracking.<field>`
/// 2. `<ref>.dependencyTracking.<field>` (as-is)
fn resolve_flake(reference: &str, explicit: bool, nix_args: &[String]) -> Result<Value> {
    let field = if explicit {
        "explicitConfigValues"
    } else {
        "configValues"
    };

    // Parse "flake#attr" form
    if let Some((flake, attr)) = reference.split_once('#') {
        let nixos_config_ref = format!(
            "{}#nixosConfigurations.{}.dependencyTracking.{}",
            flake, attr, field
        );
        eprintln!("Evaluating {}#{}...", flake, attr);

        match nix_eval_json(&nixos_config_ref, nix_args) {
            Ok(value) => return Ok(value),
            Err(e) => eprintln!("  tried {}: {:#}", nixos_config_ref, e),
        }

        // Try as-is
        let direct_ref = format!("{}.dependencyTracking.{}", reference, field);
        match nix_eval_json(&direct_ref, nix_args) {
            Ok(value) => return Ok(value),
            Err(e) => eprintln!("  tried {}: {:#}", direct_ref, e),
        }

        bail!(
            "Could not evaluate tracking data for '{}'.\n\
             Make sure the flake's nixosSystem is called with trackDependencies = true\n\
             and uses a nixpkgs that supports dependency tracking.",
            reference
        );
    }

    bail!("Invalid flake reference: {}", reference);
}

pub struct CombinedResult {
    pub config_values: Value,
    pub filtered_deps: Value,
}

/// Resolve both config values and dependency data in a single evaluation.
///
/// For flake references this uses a single `nix eval --apply` call to avoid
/// evaluating the NixOS configuration twice.
pub fn resolve_combined(arg: &str, explicit: bool, nix_args: &[String]) -> Result<CombinedResult> {
    let path = Path::new(arg);
    if path.is_dir() {
        return resolve_combined_dir(path, explicit);
    }
    if arg.contains('#') {
        return resolve_combined_flake(arg, explicit, nix_args);
    }
    bail!(
        "'{}' is not an existing directory or flake reference (flake refs must contain '#')",
        arg
    );
}

fn resolve_combined_dir(dir: &Path, explicit: bool) -> Result<CombinedResult> {
    let config_values = resolve_dir(dir, explicit)?;
    let filtered_deps = resolve_deps_dir(dir).unwrap_or(Value::Array(vec![]));
    Ok(CombinedResult {
        config_values,
        filtered_deps,
    })
}

fn resolve_combined_flake(reference: &str, explicit: bool, nix_args: &[String]) -> Result<CombinedResult> {
    let field = if explicit {
        "explicitConfigValues"
    } else {
        "configValues"
    };

    if let Some((flake, attr)) = reference.split_once('#') {
        let nixos_config_ref = format!(
            "{}#nixosConfigurations.{}.dependencyTracking",
            flake, attr
        );
        let apply_expr = format!(
            "t: {{ configValues = t.{}; filteredDeps = t.filteredDeps; }}",
            field
        );
        eprintln!("Evaluating {}#{}...", flake, attr);

        match nix_eval_apply_json(&nixos_config_ref, &apply_expr, nix_args) {
            Ok(value) => {
                let config_values = value.get("configValues").cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                let filtered_deps = value.get("filteredDeps").cloned()
                    .unwrap_or(Value::Array(vec![]));
                return Ok(CombinedResult { config_values, filtered_deps });
            }
            Err(e) => eprintln!("  tried {} --apply: {:#}", nixos_config_ref, e),
        }

        // Fallback: try as-is
        let direct_ref = format!("{}.dependencyTracking", reference);
        let apply_expr = format!(
            "t: {{ configValues = t.{}; filteredDeps = t.filteredDeps; }}",
            field
        );
        match nix_eval_apply_json(&direct_ref, &apply_expr, nix_args) {
            Ok(value) => {
                let config_values = value.get("configValues").cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                let filtered_deps = value.get("filteredDeps").cloned()
                    .unwrap_or(Value::Array(vec![]));
                return Ok(CombinedResult { config_values, filtered_deps });
            }
            Err(e) => eprintln!("  tried {} --apply: {:#}", direct_ref, e),
        }

        bail!(
            "Could not evaluate tracking data for '{}'.\n\
             Make sure the flake's nixosSystem is called with trackDependencies = true\n\
             and uses a nixpkgs that supports dependency tracking.",
            reference
        );
    }

    bail!("Invalid flake reference: {}", reference);
}

/// Run `nix eval --json <ref>` and parse the result.
fn nix_eval_json(reference: &str, extra_args: &[String]) -> Result<Value> {
    let mut cmd = Command::new("nix");
    cmd.args(["eval", "--json"]);
    cmd.args(extra_args);
    cmd.arg(reference);

    let output = cmd.output().context("failed to run nix eval")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix eval failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("nix eval output not UTF-8")?;
    serde_json::from_str(&stdout).context("parsing nix eval JSON output")
}

/// Run `nix eval --json --apply <expr> <ref>` and parse the result.
fn nix_eval_apply_json(reference: &str, apply_expr: &str, extra_args: &[String]) -> Result<Value> {
    let mut cmd = Command::new("nix");
    cmd.args(["eval", "--json", "--apply", apply_expr]);
    cmd.args(extra_args);
    cmd.arg(reference);

    let output = cmd.output().context("failed to run nix eval")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix eval failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("nix eval output not UTF-8")?;
    serde_json::from_str(&stdout).context("parsing nix eval JSON output")
}
