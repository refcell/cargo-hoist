//! CLI Logic

use crate::registry::HoistRegistry;
use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};

/// Command line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Verbosity level (0-4)
    #[arg(long, short, action = ArgAction::Count, default_value = "0")]
    pub verbosity: u8,

    /// Suppress all stdout.
    #[arg(long, short)]
    pub quiet: bool,

    /// The cargo-hoist subcommand
    #[clap(subcommand)]
    pub command: Option<Command>,
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
    let Args {
        verbosity,
        quiet,
        command,
    } = Args::parse();

    crate::telemetry::init_tracing_subscriber(verbosity)?;

    // Always attempt to install the hoist pre-hook in the user's shell
    // config file. If not present, prompt the user with a confirmation.
    HoistRegistry::create_pre_hook(true)?;

    match command {
        None => HoistRegistry::install(Vec::new(), quiet),
        Some(c) => match c {
            Command::Hoist { binaries, bins } => {
                HoistRegistry::hoist(crate::utils::merge_and_dedup_vecs(binaries, bins), quiet)
            }
            Command::Search { binary } => HoistRegistry::find(binary),
            Command::List => HoistRegistry::list(),
            Command::Register { binaries, bins } => {
                HoistRegistry::install(crate::utils::merge_and_dedup_vecs(binaries, bins), quiet)
            }
            Command::Nuke => HoistRegistry::nuke(),
        },
    }
}

#[cfg(test)]
mod tests {
    use assert_cmd::Command;
    use rand::{distributions::Alphanumeric, Rng};
    use serial_test::serial;
    use std::path::PathBuf;

    const HOIST_BIN: &str = "cargo-hoist";

    #[test]
    #[serial]
    fn test_cli_no_args() {
        let (_, _) = setup_test_dir();
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        let assert = cmd.assert();
        assert.success().stdout("");
    }

    #[test]
    #[serial]
    fn test_cli_nuke() {
        let (_, _) = setup_test_dir();
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        cmd.arg("nuke").assert().success().stdout("");
    }

    #[test]
    #[serial]
    fn test_cli_install() {
        let (_, _) = setup_test_dir();
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        cmd.arg("install").assert().success().stdout("");
    }

    #[test]
    #[serial]
    fn test_cli_list() {
        let (_, _) = setup_test_dir();
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        cmd.arg("list").assert().success();
    }

    #[test]
    #[serial]
    fn test_cli_unrecognized_subcommand() {
        let (_, _) = setup_test_dir();
        let mut cmd = Command::cargo_bin(HOIST_BIN).unwrap();
        let assert = cmd.arg("foobar").assert();
        assert.failure().code(2).stderr(
            r#"error: unrecognized subcommand 'foobar'

Usage: cargo-hoist [OPTIONS] [COMMAND]

For more information, try '--help'.
"#,
        );
    }

    /// Helper function to setup a batteries included [TempDir].
    fn setup_test_dir() -> (PathBuf, tempfile::TempDir) {
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = tempdir.path().join(s);
        std::fs::create_dir(&test_tempdir).unwrap();
        // copy the cargo hoist bin to the tempdir
        let hoist_bin = PathBuf::from("target/release/cargo-hoist");
        let hoist_bin_dest = test_tempdir.join(HOIST_BIN);
        std::fs::copy(hoist_bin, hoist_bin_dest).unwrap();
        let s: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(7)
            .map(char::from)
            .collect();
        std::env::set_current_dir(&test_tempdir).unwrap();
        (test_tempdir, tempdir)
    }
}
