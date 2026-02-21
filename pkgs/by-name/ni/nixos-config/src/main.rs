mod diff;
mod json2nix;
mod resolve;
mod save;
mod show;
mod theme;
mod tree;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

/// View, diff, and save NixOS tracked configurations.
#[derive(Parser)]
#[command(name = "nixos-config", version)]
struct Cli {
    /// Extra arguments passed to nix commands (e.g. --nix-arg=--impure)
    #[arg(long = "nix-arg", global = true)]
    nix_args: Vec<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Diff two NixOS configurations
    Diff {
        /// Use tracking-explicit.json (only explicitly defined values)
        #[arg(long)]
        explicit: bool,

        /// Run an external diff command instead of the built-in viewer
        #[arg(long)]
        exec: Option<String>,

        /// Old configuration (path or flake ref); defaults to /var/run/current-system
        #[arg(name = "OLD")]
        old: Option<String>,

        /// New configuration (path or flake ref)
        #[arg(name = "NEW")]
        new: Option<String>,
    },

    /// Show a NixOS configuration
    Show {
        /// Use tracking-explicit.json (only explicitly defined values)
        #[arg(long)]
        explicit: bool,

        /// Use flat dot-path format (no nested braces)
        #[arg(long)]
        flat: bool,

        /// Configuration to show (path or flake ref); defaults to /var/run/current-system
        #[arg(name = "CONFIG")]
        config: Option<String>,
    },

    /// Save a NixOS configuration to a .nix file
    Save {
        /// Use tracking-explicit.json (only explicitly defined values)
        #[arg(long)]
        explicit: bool,

        /// Use flat dot-path format (no nested braces)
        #[arg(long)]
        flat: bool,

        /// Output file path
        #[arg(name = "OUT")]
        output: String,

        /// Configuration to save (path or flake ref); defaults to /var/run/current-system
        #[arg(name = "CONFIG")]
        config: Option<String>,
    },

    /// Browse a NixOS configuration as an interactive tree
    Tree {
        /// Use tracking-explicit.json (only explicitly defined values)
        #[arg(long)]
        explicit: bool,

        /// Color output mode
        #[arg(long, value_enum, default_value_t = ColorMode::Auto)]
        color: ColorMode,

        /// Configuration to browse (path or flake ref); defaults to /var/run/current-system
        #[arg(name = "CONFIG")]
        config: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ColorMode {
    Always,
    Auto,
    Never,
}

const DEFAULT_CONFIG: &str = "/var/run/current-system";

fn main() -> Result<()> {
    let cli = Cli::parse();
    let nix_args = &cli.nix_args;

    match cli.command {
        Commands::Diff {
            explicit,
            exec,
            old,
            new,
        } => {
            // With two positional args: OLD NEW
            // With one positional arg: NEW (OLD defaults to current-system)
            let (old_arg, new_arg) = match (old, new) {
                (Some(o), Some(n)) => (o, n),
                (Some(n), None) => (DEFAULT_CONFIG.to_string(), n),
                (None, None) => {
                    anyhow::bail!("diff requires at least one argument (NEW config)");
                }
                _ => unreachable!(),
            };
            diff::run(&old_arg, &new_arg, explicit, exec.as_deref(), nix_args)
        }
        Commands::Show {
            explicit,
            flat,
            config,
        } => {
            let config = config.as_deref().unwrap_or(DEFAULT_CONFIG);
            show::run(config, explicit, flat, nix_args)
        }
        Commands::Save {
            explicit,
            flat,
            output,
            config,
        } => {
            let config = config.as_deref().unwrap_or(DEFAULT_CONFIG);
            save::run(&output, config, explicit, flat, nix_args)
        }
        Commands::Tree {
            explicit,
            color,
            config,
        } => {
            let use_color = match color {
                ColorMode::Always => true,
                ColorMode::Never => false,
                ColorMode::Auto => tui::is_tty(),
            };
            let config = config.as_deref().unwrap_or(DEFAULT_CONFIG);
            tree::run(config, explicit, use_color, nix_args)
        }
    }
}
