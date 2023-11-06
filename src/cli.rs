//! CLI Logic

use crate::registry::HoistRegistry;
use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};

#[derive(Debug, Parser)]
#[clap(name = "cargo-hoist", author, bin_name = "cargo", version)]
enum Cargo {
    #[clap(alias = "h")]
    Hoist(Args),
}

/// Command line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Global options
    #[clap(flatten)]
    pub globals: GlobalOpts,

    /// The cargo-hoist subcommand
    #[clap(subcommand)]
    pub command: Option<Command>,
}

/// Global Config Options
#[derive(Debug, clap::Args)]
pub struct GlobalOpts {
    /// Verbosity level (0-4). Default: 0 (ERROR).
    #[arg(long, short, action = ArgAction::Count, default_value = "0")]
    pub verbosity: u8,

    /// Suppress all stdout.
    #[arg(long, short)]
    pub quiet: bool,
}

/// Subcommands
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Hoist dependencies
    Hoist {
        /// An optional list of binaries to bring into scope from the hoist toml registry
        bins: Option<Vec<String>>,

        /// Binary list flag. Merged ad de-duplicated with any binaries provided in the inline
        /// argument.
        #[clap(short, long)]
        binaries: Option<Vec<String>>,
    },
    /// List registered dependencies.
    List,
    /// Search for a binary in the hoist toml registry.
    #[clap(alias = "find")]
    Search {
        /// The binary to search for in the hoist toml registry.
        binary: String,
    },
    /// Nuke wipes the hoist toml registry.
    Nuke,
    /// Registers a binary in the global hoist toml registry
    #[clap(alias = "install")]
    Register {
        /// An optional list of binaries to install in the hoist toml registry
        bins: Option<Vec<String>>,

        /// Binary list flag. Merged ad de-duplicated with any binaries provided in the inline
        /// argument.
        #[clap(short, long)]
        binaries: Option<Vec<String>>,
    },
}

/// Run the main hoist command
pub fn run() -> Result<()> {
    let Cargo::Hoist(arg) = Cargo::parse();

    crate::telemetry::init_tracing_subscriber(arg.globals.verbosity)?;

    HoistRegistry::create_pre_hook(true, false)?;

    match arg.command {
        None => HoistRegistry::install(None, Vec::new(), arg.globals.quiet),
        Some(c) => match c {
            Command::Hoist { binaries, bins } => HoistRegistry::hoist(
                crate::utils::merge_and_dedup_vecs(binaries, bins),
                arg.globals.quiet,
            ),
            Command::Search { binary } => HoistRegistry::find(binary),
            Command::List => HoistRegistry::list(false),
            Command::Register { binaries, bins } => HoistRegistry::install(
                None,
                crate::utils::merge_and_dedup_vecs(binaries, bins),
                arg.globals.quiet,
            ),
            Command::Nuke => HoistRegistry::nuke(false),
        },
    }
}

#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use rand::{distributions::Alphanumeric, Rng};
    use serial_test::serial;
    use std::path::PathBuf;
    use tempfile::TempDir;

    const HOIST_BIN: &str = "cargo-hoist";

    #[test]
    #[serial]
    fn test_cli_no_args() {
        let tempdir = tempfile::tempdir().unwrap();
        let _ = setup_test_dir(&tempdir);
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        let assert = cmd.arg("hoist").assert();
        assert.success().stdout("");
    }

    #[test]
    #[serial]
    fn test_cli_nuke() {
        let tempdir = tempfile::tempdir().unwrap();
        let _ = setup_test_dir(&tempdir);
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        cmd.arg("hoist").arg("nuke").assert().success().stdout("");
    }

    #[test]
    #[serial]
    fn test_cli_install() {
        let tempdir = tempfile::tempdir().unwrap();
        let _ = setup_test_dir(&tempdir);
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        cmd.arg("hoist")
            .arg("install")
            .assert()
            .success()
            .stdout("");
    }

    #[test]
    #[serial]
    fn test_cli_list() {
        let tempdir = tempfile::tempdir().unwrap();
        let _ = setup_test_dir(&tempdir);
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        cmd.arg("hoist").arg("list").assert().success();
    }

    #[test]
    #[serial]
    fn test_cli_unrecognized_subcommand() {
        let tempdir = tempfile::tempdir().unwrap();
        let _ = setup_test_dir(&tempdir);
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        let assert = cmd.arg("hoist").arg("foobar").assert();
        assert.failure().code(2).stderr(
            r#"error: unrecognized subcommand 'foobar'

Usage: cargo hoist [OPTIONS] [COMMAND]

For more information, try '--help'.
"#,
        );
    }

    /// Helper function to setup a batteries included [TempDir].
    fn setup_test_dir(tempdir: &TempDir) -> PathBuf {
        // Create the test tempdir
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        let test_tempdir = tempdir.path().join(s);
        std::fs::create_dir(&test_tempdir).unwrap();

        // Try to copy the cargo-hoist binary from the target/debug/
        // directory, falling back to a manual install if not present.
        let backup = match std::env::current_dir() {
            Ok(d) => {
                if d.join("target/debug/cargo-hoist").exists() {
                    std::fs::copy(
                        d.join("target/debug/cargo-hoist"),
                        test_tempdir.join("cargo-hoist"),
                    )
                    .unwrap();
                    false
                } else {
                    true
                }
            }
            Err(_) => true,
        };
        if backup {
            let _ = std::process::Command::new("cargo")
                .args(["install", "--path", "."])
                .current_dir(&test_tempdir)
                .output()
                .unwrap();
        }

        // Set the current directory to the test tempdir and return the
        // test tempdir and the tempdir.
        std::env::set_current_dir(&test_tempdir).unwrap();

        test_tempdir
    }
}
