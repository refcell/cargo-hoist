use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};
use inquire::Confirm;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::io::Read;
use std::io::Write;
use std::path::PathBuf;
use tracing::Level;

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
        #[clap(long, short)]
        binaries: Option<Vec<String>>,
    },
    /// List hoisted dependencies
    List,
    /// Installs a binary in the global hoist toml registry
    Install {
        /// An optional list of binaries to install in the hoist toml registry
        #[clap(long, short)]
        binaries: Option<Vec<String>>,
    },
}

/// The bash function to install the hoist cargo pre-hook.
pub const INSTALL_BASH_FUNCTION: &str = r#"
function cargo() {
    if ~/.cargo/bin/cargo hoist --help &>/dev/null; then
      ~/.cargo/bin/cargo hoist install
    fi
    ~/.cargo/bin/cargo "$@"
}
"#;

/// Run the main hoist command
pub fn run() -> Result<()> {
    let Args { verbosity, command } = Args::parse();
    init_tracing_subscriber(verbosity)?;

    // On first run, we want to install the hoist pre-hook in the user's bash file.
    // So we want to create it using inquire's confirm prompt.
    HoistRegistry::create_pre_hook(true)?;

    // Match on the subcommand and run hoist.
    match command {
        None => HoistRegistry::install(None),
        Some(c) => match c {
            Command::Hoist { binaries } => HoistRegistry::hoist(binaries),
            Command::List => HoistRegistry::list(),
            Command::Install { binaries } => HoistRegistry::install(binaries),
        },
    }
}

/// Binary Metadata Object
#[derive(Debug, Default, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct HoistedBinary {
    /// The binary name
    pub name: String,
    /// The binary location
    pub location: PathBuf,
}

impl HoistedBinary {
    /// Creates a new hoisted binary.
    pub fn new(name: String, location: PathBuf) -> Self {
        Self { name, location }
    }
}

/// Hoist Registry
///
/// The global hoist registry is stored in ~/.hoist/registry.toml
/// and contains the memoized list of binaries that have been
/// built with cargo and saved as [HoistedBinary] objects.
#[derive(Debug, Default, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct HoistRegistry {
    /// The list of hoisted binaries.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub binaries: HashSet<HoistedBinary>,
}
//
// impl From<Vec<HoistedBinary>> for HoistRegistry {
//     fn from(binaries: Vec<HoistedBinary>) -> Self {
//         HoistRegistry { binaries }
//     }
// }

impl HoistRegistry {
    /// The path to the hoist directory.
    pub fn dir() -> Result<PathBuf> {
        let hoist_dir = std::env::var("HOME")? + "/.hoist/";
        Ok(PathBuf::from(hoist_dir))
    }

    /// The path to the hoist registry file.
    pub fn path() -> Result<PathBuf> {
        let hoist_dir = HoistRegistry::dir()?;
        Ok(hoist_dir.join("registry.toml"))
    }

    /// Hook identifier file.
    /// This is used to indicate that the hoist pre-hook has been installed.
    pub fn hook_identifier() -> Result<PathBuf> {
        let hoist_dir = HoistRegistry::dir()?;
        Ok(hoist_dir.join("hook"))
    }

    /// Create the hoist directory if it doesn't exist.
    pub fn create_dir() -> Result<()> {
        let hoist_dir = HoistRegistry::dir()?;
        if !std::path::Path::new(&hoist_dir).exists() {
            tracing::info!("Creating ~/.hoist/ directory");
            std::fs::create_dir(hoist_dir)?;
        }
        Ok(())
    }

    /// Create the hoist registry file.
    pub fn create_registry() -> Result<()> {
        HoistRegistry::create_dir()?;
        let registry_file = HoistRegistry::path()?;
        if !std::path::Path::new(&registry_file).exists() {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(registry_file)?;
            let default_registry = HoistRegistry::default();
            let toml = toml::to_string(&default_registry)?;
            file.write_all(toml.as_bytes())?;
        }
        Ok(())
    }

    /// Create the hoist pre-hook in the user bash file.
    pub fn create_pre_hook(with_confirm: bool) -> Result<()> {
        HoistRegistry::create_dir()?;
        let hook_file = HoistRegistry::hook_identifier()?;
        if !std::path::Path::new(&hook_file).exists() {
            if with_confirm && !Confirm::new("Cargo hoist pre-cargo hook not installed. Do you want to install? ([y]/n) Once installed, this prompt will not bother you again :)").prompt()? {
                anyhow::bail!("cargo hoist installation rejected");
            }
            // Write the bash function to the user's bash file.
            let shell_config = get_shell_config_file(detect_shell()?)?;
            if !shell_config.as_path().exists() {
                anyhow::bail!("~/.bashrc file does not exist");
            }
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(shell_config)?;
            file.write_all(INSTALL_BASH_FUNCTION.as_bytes())?;

            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(hook_file)?;
            file.write_all("hook".as_bytes())?;
        }
        Ok(())
    }

    /// Installs the hoist registry to a `.hoist/` subdir in the
    /// user's home directory.
    pub fn setup() -> Result<()> {
        HoistRegistry::create_dir()?;
        HoistRegistry::create_registry()?;
        HoistRegistry::create_pre_hook(false)?;
        Ok(())
    }

    /// Installs binaries in the hoist toml registry.
    pub fn install(binaries: Option<Vec<String>>) -> Result<()> {
        HoistRegistry::setup()?;

        // Then we read the registry file into a HoistRegistry object.
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(registry_file)?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let mut registry: HoistRegistry = toml::from_str(&registry_toml)?;

        // Then we iterate over the binaries and add them to the registry.
        let binaries = match binaries {
            Some(b) => b,
            None => {
                // Try to read all binaries in the target directory.
                let mut binaries = vec![];
                let target_dir = std::env::current_dir()?.join("target/release/");
                for entry in std::fs::read_dir(target_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    let bin_file_name = path
                        .file_name()
                        .ok_or(anyhow::anyhow!("[std] failed to extract binary name"))?;
                    let binary_name = bin_file_name
                        .to_str()
                        .ok_or(anyhow::anyhow!(
                            "[std] failed to convert binary path name to string"
                        ))?
                        .to_string();
                    binaries.push(binary_name);
                }
                binaries
            }
        };

        for binary in binaries {
            let binary_path = std::env::current_dir()?
                .join("target/release/")
                .join(binary);
            let binary_path = binary_path.canonicalize()?;
            let bin_file_name = binary_path
                .file_name()
                .ok_or(anyhow::anyhow!("[std] failed to extract binary name"))?;
            let binary_name = bin_file_name
                .to_str()
                .ok_or(anyhow::anyhow!(
                    "[std] failed to convert binary path name to string"
                ))?
                .to_string();
            let binary = HoistedBinary::new(binary_name, binary_path);
            registry.binaries.insert(binary);
        }

        // Then we write the registry back to the registry file.
        let toml = toml::to_string(&registry)?;
        file.write_all(toml.as_bytes())?;

        Ok(())
    }

    /// Lists the binaries in the hoist toml registry.
    pub fn list() -> Result<()> {
        HoistRegistry::setup()?;

        // Then we read the registry file into a HoistRegistry object.
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new().read(true).open(registry_file)?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let registry: HoistRegistry = toml::from_str(&registry_toml)?;

        // Then we iterate over the binaries and print them.
        for binary in registry.binaries {
            println!("{}", binary.name);
        }

        Ok(())
    }

    /// Hoists binaries from the hoist toml registry into scope.
    pub fn hoist(binaries: Option<Vec<String>>) -> Result<()> {
        HoistRegistry::setup()?;

        // Then we read the registry file into a HoistRegistry object.
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new().read(true).open(registry_file)?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let registry: HoistRegistry = toml::from_str(&registry_toml)?;

        // Then we iterate over the binaries and print them.
        let binaries = binaries.unwrap_or_default();
        for binary in binaries {
            let binary = registry
                .binaries
                .iter()
                .find(|b| b.name == binary)
                .ok_or_else(|| anyhow::anyhow!("Binary not found in hoist registry"))?;
            let binary_path = binary.location.clone();
            let binary_path = binary_path.canonicalize()?;
            let bin_file_name = binary_path
                .file_name()
                .ok_or(anyhow::anyhow!("[std] failed to extract binary name"))?;
            let binary_name = bin_file_name
                .to_str()
                .ok_or(anyhow::anyhow!(
                    "[std] failed to convert binary path name to string"
                ))?
                .to_string();
            let binary_path = binary_path
                .to_str()
                .ok_or(anyhow::anyhow!(
                    "[std] failed to convert binary path name to string"
                ))?
                .to_string();
            println!("export PATH={}:$PATH", binary_path);
            println!("alias {}={}", binary_name, binary_name);
        }

        Ok(())
    }
}

/// The type of shell
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellType {
    /// Zsh
    Zsh,
    /// Bash
    Bash,
    /// Other
    Other,
}

/// Detect the type of shell a user is using.
pub fn detect_shell() -> Result<ShellType> {
    if let Ok(shell_path) = std::env::var("SHELL") {
        if shell_path.contains("zsh") {
            Ok(ShellType::Zsh)
        } else if shell_path.contains("bash") {
            Ok(ShellType::Bash)
        } else {
            Ok(ShellType::Other)
        }
    } else {
        Err(anyhow::anyhow!("Unable to determine the user's shell."))
    }
}

/// Helper to get the path to the user's shell config file.
pub fn get_shell_config_file(shell_type: ShellType) -> Result<PathBuf> {
    let home_dir = std::env::var("HOME")?;
    match shell_type {
        ShellType::Zsh => Ok(PathBuf::from(format!("{}/.zshrc", home_dir))),
        _ => Ok(PathBuf::from(format!("{}/.bashrc", home_dir))),
        // ShellType::Other => Err(anyhow::anyhow!("Unsupported shell type.")),
    }
}

/// Initializes the tracing subscriber.
///
/// The verbosity level determines the maximum level of tracing.
/// - 0: ERROR
/// - 1: WARN
/// - 2: INFO
/// - 3: DEBUG
/// - 4+: TRACE
///
/// # Arguments
/// * `verbosity_level` - The verbosity level (0-4)
///
/// # Returns
/// * `Result<()>` - Ok if successful, Err otherwise.
pub fn init_tracing_subscriber(verbosity_level: u8) -> Result<()> {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(match verbosity_level {
            0 => Level::ERROR,
            1 => Level::WARN,
            2 => Level::INFO,
            3 => Level::DEBUG,
            _ => Level::TRACE,
        })
        .finish();
    tracing::subscriber::set_global_default(subscriber).map_err(|e| anyhow::anyhow!(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

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
        let binary1 = target_dir.join("binary1");
        std::fs::File::create(&binary1).unwrap();
        let binary2 = target_dir.join("binary2");
        std::fs::File::create(&binary2).unwrap();
        let original_home = std::env::var_os("HOME").unwrap();
        std::env::set_var("HOME", test_tempdir);

        HoistRegistry::install(None).unwrap();

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
}
