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
pub fn resolve(arg: &str, explicit: bool) -> Result<Value> {
    let path = Path::new(arg);
    if path.is_dir() {
        return resolve_dir(path, explicit);
    }
    if arg.contains('#') {
        return resolve_flake(arg, explicit);
    }
    bail!(
        "'{}' is not an existing directory or flake reference (flake refs must contain '#')",
        arg
    );
}

/// Resolve a config argument to dependency tracking data (filteredDeps).
#[allow(dead_code)]
pub fn resolve_deps(arg: &str) -> Result<Value> {
    let path = Path::new(arg);
    if path.is_dir() {
        return resolve_deps_dir(path);
    }
    if arg.contains('#') {
        return resolve_deps_flake(arg);
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

/// Resolve dependency data from a flake reference.
fn resolve_deps_flake(reference: &str) -> Result<Value> {
    if let Some((flake, attr)) = reference.split_once('#') {
        let nixos_config_ref = format!(
            "{}#nixosConfigurations.{}.dependencyTracking.filteredDeps",
            flake, attr
        );
        eprintln!("Evaluating deps for {}#{}...", flake, attr);

        if let Ok(value) = nix_eval_json(&nixos_config_ref) {
            return Ok(value);
        }

        let direct_ref = format!("{}.dependencyTracking.filteredDeps", reference);
        if let Ok(value) = nix_eval_json(&direct_ref) {
            return Ok(value);
        }

        // Last resort: build and read from output
        eprintln!("Falling back to nix build...");
        let output = Command::new("nix")
            .args(["build", "--no-link", "--print-out-paths", reference])
            .output()
            .context("failed to run nix build")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("nix build failed: {}", stderr.trim());
        }

        let stdout = String::from_utf8(output.stdout).context("nix build output not UTF-8")?;
        let out_path = stdout.trim();
        return resolve_deps_dir(Path::new(out_path));
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
/// 3. Fallback: `nix build --print-out-paths` and read from output dir
fn resolve_flake(reference: &str, explicit: bool) -> Result<Value> {
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

        if let Ok(value) = nix_eval_json(&nixos_config_ref) {
            return Ok(value);
        }

        // Try as-is
        let direct_ref = format!("{}.dependencyTracking.{}", reference, field);
        if let Ok(value) = nix_eval_json(&direct_ref) {
            return Ok(value);
        }

        // Last resort: build and read from output
        eprintln!("Falling back to nix build...");
        return resolve_flake_build(reference, explicit);
    }

    bail!("Invalid flake reference: {}", reference);
}

/// Run `nix eval --json <ref>` and parse the result.
fn nix_eval_json(reference: &str) -> Result<Value> {
    let output = Command::new("nix")
        .args(["eval", "--json", reference])
        .output()
        .context("failed to run nix eval")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix eval failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("nix eval output not UTF-8")?;
    serde_json::from_str(&stdout).context("parsing nix eval JSON output")
}

/// Build a flake output and read tracking JSON from the result.
fn resolve_flake_build(reference: &str, explicit: bool) -> Result<Value> {
    let output = Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths", reference])
        .output()
        .context("failed to run nix build")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix build failed: {}", stderr.trim());
    }

    let stdout = String::from_utf8(output.stdout).context("nix build output not UTF-8")?;
    let out_path = stdout.trim();
    resolve_dir(Path::new(out_path), explicit)
}
