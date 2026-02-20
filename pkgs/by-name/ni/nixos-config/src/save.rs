use std::io::Write;

use anyhow::{Context, Result};

use crate::json2nix;
use crate::resolve;

pub fn run(output_path: &str, config: &str, explicit: bool, flat: bool) -> Result<()> {
    let json = resolve::resolve(config, explicit)?;
    let nix_text = json2nix::convert(&json, flat);

    if output_path == "/dev/stdout" || output_path == "-" {
        std::io::stdout()
            .write_all(nix_text.as_bytes())
            .context("writing to stdout")?;
    } else {
        std::fs::write(output_path, &nix_text)
            .with_context(|| format!("writing to {}", output_path))?;
        eprintln!("Wrote {}", output_path);
    }
    Ok(())
}
