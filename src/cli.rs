//! CLI Logic

use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};

use crate::registry::HoistRegistry;

/// Command line arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Verbosity level (0-4)
    #[arg(long, short, action = ArgAction::Count, default_value = "0")]
    pub verbosity: u8,

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
    let Args { verbosity, command } = Args::parse();

    crate::telemetry::init_tracing_subscriber(verbosity)?;

    // Always attempt to install the hoist pre-hook in the user's shell
    // config file. If not present, prompt the user with a confirmation.
    HoistRegistry::create_pre_hook(true)?;

    match command {
        None => HoistRegistry::install(Vec::new()),
        Some(c) => match c {
            Command::Hoist { binaries, bins } => {
                HoistRegistry::hoist(crate::utils::merge_and_dedup_vecs(binaries, bins))
            }
            Command::Search { binary } => HoistRegistry::find(binary),
            Command::List => HoistRegistry::list(),
            Command::Register { binaries, bins } => {
                HoistRegistry::install(crate::utils::merge_and_dedup_vecs(binaries, bins))
            }
            Command::Nuke => HoistRegistry::nuke(),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::prelude::OpenOptionsExt;

    use super::*;
    use crate::binaries::HoistedBinary;
    use crate::shell::*;
    use serial_test::serial;
    use std::collections::HashSet;
    use std::io::Read;

    #[test]
    #[serial]
    fn test_setup() {
        // Create a tempdir and set it as the current working directory
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = tempdir.path().join("test_setup");
        std::fs::create_dir(&test_tempdir).unwrap();
        std::env::set_current_dir(&test_tempdir).unwrap();
        let bash_file = test_tempdir.join(".bashrc");
        std::fs::File::create(&bash_file).unwrap();
        let zshrc = test_tempdir.join(".zshrc");
        std::fs::File::create(&zshrc).unwrap();
        let original_home = std::env::var_os("HOME").unwrap();
        std::env::set_var("HOME", test_tempdir);

        HoistRegistry::setup().unwrap();

        let hoist_dir = HoistRegistry::dir().unwrap();
        assert!(std::path::Path::new(&hoist_dir).exists());
        let registry_file = HoistRegistry::path().unwrap();
        assert!(std::path::Path::new(&registry_file).exists());
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .open(registry_file)
            .unwrap();
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml).unwrap();
        let registry: HoistRegistry = toml::from_str(&registry_toml).unwrap();
        assert_eq!(registry, HoistRegistry::default());
        let hook_file = HoistRegistry::hook_identifier().unwrap();
        assert!(std::path::Path::new(&hook_file).exists());
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .open(bash_file)
            .unwrap();
        let mut bash_file_contents = String::new();
        file.read_to_string(&mut bash_file_contents).unwrap();

        // If the bash file is empty, try to read the zshrc file.
        if bash_file_contents.is_empty() {
            let mut file = std::fs::OpenOptions::new().read(true).open(zshrc).unwrap();
            let mut zshrc_file_contents = String::new();
            file.read_to_string(&mut zshrc_file_contents).unwrap();
            assert_eq!(zshrc_file_contents, INSTALL_BASH_FUNCTION);
        } else {
            assert_eq!(bash_file_contents, INSTALL_BASH_FUNCTION);
        }

        // Restore the original HOME directory.
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_install() {
        // Populate the temporary directory.
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = tempdir.path().join("test_install");
        std::fs::create_dir(&test_tempdir).unwrap();
        std::env::set_current_dir(&test_tempdir).unwrap();
        let bash_file = test_tempdir.join(".bashrc");
        std::fs::File::create(&bash_file).unwrap();
        let zshrc = test_tempdir.join(".zshrc");
        std::fs::File::create(&zshrc).unwrap();
        let target_dir = test_tempdir.join("target/release/");
        std::fs::create_dir_all(&target_dir).unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary1"))
            .unwrap();
        opts.sync_all().unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary2"))
            .unwrap();
        opts.sync_all().unwrap();

        let original_home = std::env::var_os("HOME").unwrap();
        std::env::set_var("HOME", test_tempdir);

        HoistRegistry::install(Vec::new()).unwrap();

        let registry_file = HoistRegistry::path().unwrap();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .open(registry_file)
            .unwrap();
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml).unwrap();
        let registry: HoistRegistry = toml::from_str(&registry_toml).unwrap();
        assert_eq!(
            registry,
            HoistRegistry {
                binaries: HashSet::from([
                    HoistedBinary::new(
                        "binary1".to_string(),
                        target_dir.join("binary1").canonicalize().unwrap()
                    ),
                    HoistedBinary::new(
                        "binary2".to_string(),
                        target_dir.join("binary2").canonicalize().unwrap()
                    ),
                ])
            }
        );

        // Restore the original HOME directory.
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_multiple_installs() {
        // Populate the temporary directory.
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = tempdir.path().join("test_multiple_installs");
        std::fs::create_dir(&test_tempdir).unwrap();
        std::env::set_current_dir(&test_tempdir).unwrap();
        let bash_file = test_tempdir.join(".bashrc");
        std::fs::File::create(&bash_file).unwrap();
        let zshrc = test_tempdir.join(".zshrc");
        std::fs::File::create(&zshrc).unwrap();
        let target_dir = test_tempdir.join("target/release/");
        std::fs::create_dir_all(&target_dir).unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary1"))
            .unwrap();
        opts.sync_all().unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary2"))
            .unwrap();
        opts.sync_all().unwrap();

        let original_home = std::env::var_os("HOME").unwrap();
        std::env::set_var("HOME", test_tempdir);

        HoistRegistry::install(Vec::new()).unwrap();
        HoistRegistry::install(Vec::new()).unwrap();
        HoistRegistry::install(Vec::new()).unwrap();
        HoistRegistry::install(Vec::new()).unwrap();

        let registry_file = HoistRegistry::path().unwrap();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .open(registry_file)
            .unwrap();
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml).unwrap();
        let registry: HoistRegistry = toml::from_str(&registry_toml).unwrap();
        assert_eq!(
            registry,
            HoistRegistry {
                binaries: HashSet::from([
                    HoistedBinary::new(
                        "binary1".to_string(),
                        target_dir.join("binary1").canonicalize().unwrap()
                    ),
                    HoistedBinary::new(
                        "binary2".to_string(),
                        target_dir.join("binary2").canonicalize().unwrap()
                    ),
                ])
            }
        );

        // Restore the original HOME directory.
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_hoist() {
        // Populate the temporary directory.
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = tempdir.path().join("test_hoist");
        std::fs::create_dir(&test_tempdir).unwrap();
        std::env::set_current_dir(&test_tempdir).unwrap();
        let bash_file = test_tempdir.join(".bashrc");
        std::fs::File::create(&bash_file).unwrap();
        let zshrc = test_tempdir.join(".zshrc");
        std::fs::File::create(&zshrc).unwrap();
        let target_dir = test_tempdir.join("target/release/");
        std::fs::create_dir_all(&target_dir).unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary1"))
            .unwrap();
        opts.sync_all().unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary2"))
            .unwrap();
        opts.sync_all().unwrap();

        // Install the binaries in the hoist registry.
        let original_home = std::env::var_os("HOME").unwrap();
        std::env::set_var("HOME", test_tempdir);
        HoistRegistry::install(Vec::new()).unwrap();

        // Hoist binary1 and not binary2 into the current directory.
        HoistRegistry::hoist(vec!["binary1".to_string()]).unwrap();
        HoistRegistry::hoist(vec!["binary1".to_string()]).unwrap();

        // Check that binary1 was hoisted.
        let binary1 = std::env::current_dir().unwrap().join("binary1");
        assert!(std::path::Path::new(&binary1).exists());
        let binary2 = std::env::current_dir().unwrap().join("binary2");
        assert!(!std::path::Path::new(&binary2).exists());

        // Restore the original HOME directory.
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_nuke() {
        // Populate the temporary directory.
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = tempdir.path().join("test_nuke");
        std::fs::create_dir(&test_tempdir).unwrap();
        std::env::set_current_dir(&test_tempdir).unwrap();
        let bash_file = test_tempdir.join(".bashrc");
        std::fs::File::create(&bash_file).unwrap();
        let zshrc = test_tempdir.join(".zshrc");
        std::fs::File::create(&zshrc).unwrap();
        let target_dir = test_tempdir.join("target/release/");
        std::fs::create_dir_all(&target_dir).unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary1"))
            .unwrap();
        opts.sync_all().unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(target_dir.join("binary2"))
            .unwrap();
        opts.sync_all().unwrap();

        // Install the binaries in the hoist registry.
        let original_home = std::env::var_os("HOME").unwrap();
        std::env::set_var("HOME", test_tempdir);
        HoistRegistry::install(Vec::new()).unwrap();

        // Nuke the hoist registry.
        HoistRegistry::nuke().unwrap();

        // Check that the registry is empty.
        let registry_file = HoistRegistry::path().unwrap();
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .open(registry_file)
            .unwrap();
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml).unwrap();
        let registry: HoistRegistry = toml::from_str(&registry_toml).unwrap();
        assert_eq!(registry, HoistRegistry::default());

        // Restore the original HOME directory.
        std::env::set_var("HOME", original_home);
    }
}
