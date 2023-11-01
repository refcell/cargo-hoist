use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};
use inquire::Confirm;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use tracing::{instrument, Level};

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
    tracing::debug!("Gracefully creating pre hook");
    HoistRegistry::create_pre_hook(true)?;

    // Match on the subcommand and run hoist.
    tracing::debug!("Running command {:?}", command);
    match command {
        None => HoistRegistry::install(Vec::new()),
        Some(c) => match c {
            Command::Hoist { binaries, bins } => {
                HoistRegistry::hoist(merge_and_dedup_vecs(binaries, bins))
            }
            Command::Search { binary } => HoistRegistry::find(binary),
            Command::List => HoistRegistry::list(),
            Command::Register { binaries, bins } => {
                HoistRegistry::install(merge_and_dedup_vecs(binaries, bins))
            }
            Command::Nuke => HoistRegistry::nuke(),
        },
    }
}

/// Helper function to merge two optional string vectors and dedup any duplicate entries.
pub fn merge_and_dedup_vecs<T: Eq + Hash + Clone + Ord>(
    a: Option<Vec<T>>,
    b: Option<Vec<T>>,
) -> Vec<T> {
    let mut merged = vec![];
    if let Some(a) = a {
        merged.extend(a);
    }
    if let Some(b) = b {
        merged.extend(b);
    }
    merged.sort();
    merged.dedup();
    merged
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
    #[instrument(skip(name, location))]
    pub fn new(name: String, location: PathBuf) -> Self {
        Self { name, location }
    }

    /// Copies the binary to the current directory.
    #[instrument]
    pub fn copy_to_current_dir(&self) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        let binary_path = current_dir.join(&self.name);
        tracing::debug!("Copying binary to current directory: {:?}", binary_path);
        std::fs::copy(&self.location, binary_path)?;
        Ok(())
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

impl HoistRegistry {
    /// Inserts a [HoistedBinary] into the registry.
    /// Will not insert if the binary already exists in the registry.
    #[instrument(skip(self, binary))]
    pub fn insert(&mut self, binary: HoistedBinary) {
        if !self.binaries.contains(&binary) {
            tracing::debug!("Binary not found in registry. Inserting.");
            self.binaries.insert(binary);
        }
    }

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
    #[instrument]
    pub fn setup() -> Result<()> {
        HoistRegistry::create_dir()?;
        HoistRegistry::create_registry()?;
        HoistRegistry::create_pre_hook(false)?;
        Ok(())
    }

    /// Returns the fully qualified path for a given binary
    /// if it is an executable.
    #[instrument]
    pub fn exec_path(exec: &Path) -> Result<String> {
        let is_file = std::fs::metadata(exec)?.is_file();
        let is_exec = std::fs::metadata(exec)?.permissions().mode() & 0o111 != 0;
        if !is_file || !is_exec {
            anyhow::bail!("{} is not executable", exec.display());
        }
        let bin_file_name = exec
            .file_name()
            .ok_or(anyhow::anyhow!("[std] failed to extract binary name"))?;
        let binary_name = bin_file_name
            .to_str()
            .ok_or(anyhow::anyhow!(
                "[std] failed to convert binary path name to string"
            ))?
            .to_string();
        tracing::debug!("retrieved binary name: {}", binary_name);
        Ok(binary_name)
    }

    /// Attempt to grab built binaries from the target directory.
    #[instrument]
    pub fn grab_binaries() -> Result<Vec<String>> {
        let target_dir = std::env::current_dir()?.join("target/release/");
        tracing::debug!("Parsing binaries in target directory: {:?}", target_dir);
        let mut binaries = vec![];
        for entry in std::fs::read_dir(target_dir)? {
            let Ok(e) = entry else {
                tracing::warn!("Failed to read entry: {:?}", entry);
                continue;
            };
            let Ok(exec) = HoistRegistry::exec_path(&e.path()) else {
                tracing::warn!("Failed to get exec path: {:?}", e);
                continue;
            };
            tracing::debug!("Found binary: {}", exec);
            binaries.push(exec);
        }
        tracing::debug!("Returning {} binaries", binaries.len());
        Ok(binaries)
    }

    /// Nukes the hoist toml registry.
    /// This writes an empty registry to the registry file.
    #[instrument]
    pub fn nuke() -> Result<()> {
        HoistRegistry::setup()?;
        let registry_file = HoistRegistry::path()?;
        // Clear the file before writing the empty registry.
        std::fs::File::create(&registry_file)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(registry_file)?;
        let registry = HoistRegistry::default();
        let toml = toml::to_string(&registry)?;
        file.write_all(toml.as_bytes())?;
        tracing::info!("Successfully nuked the hoist registry");
        Ok(())
    }

    /// Installs binaries in the hoist toml registry.
    #[instrument(skip(binaries))]
    pub fn install(binaries: Vec<String>) -> Result<()> {
        HoistRegistry::setup()?;
        tracing::debug!("Installing binaries: {:?}", binaries);

        // Then we read the registry file into a HoistRegistry object.
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&registry_file)?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let mut registry: HoistRegistry = toml::from_str(&registry_toml)?;
        tracing::debug!("Registry: {:?}", registry);

        // Then we iterate over the binaries and add them to the registry.
        let binaries = if binaries.is_empty() {
            HoistRegistry::grab_binaries().unwrap_or_default()
        } else {
            binaries
        };

        tracing::debug!("Hoisting {} binaries", binaries.len());
        for binary in &binaries {
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
            tracing::debug!("Hoisted binary: {}", binary_name);
            let binary = HoistedBinary::new(binary_name, binary_path);
            registry.insert(binary);
        }

        // Only perform a writeback if there are binaries to hoist.
        match binaries.len() {
            0 => tracing::warn!("No binaries found in the target directory"),
            _ => {
                // first write no bytes to wipe the registry file
                let mut file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&registry_file)?;
                let toml = toml::to_string(&registry)?;
                let _ = file.write(toml.as_bytes())?;
                file.flush()?;
                tracing::info!("Successfully installed binaries to the registry")
            }
        }

        Ok(())
    }

    /// Finds a given binary in the hoist registry toml.
    #[instrument(skip(binary))]
    pub fn find(binary: impl AsRef<str>) -> Result<()> {
        HoistRegistry::setup()?;

        // Then we read the registry file into a HoistRegistry object.
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new().read(true).open(registry_file)?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let registry: HoistRegistry = toml::from_str(&registry_toml)?;

        // Find the binary in the registry.
        let binary = binary.as_ref();
        let binary = registry
            .binaries
            .iter()
            .find(|b| b.name == binary)
            .ok_or(anyhow::anyhow!("Failed to find binary in hoist registry"))?;
        HoistRegistry::print_color(&format!("{}: ", binary.name), Color::Blue, false)?;
        HoistRegistry::print_color(&binary.location.display().to_string(), Color::Cyan, true)?;
        Ok(())
    }

    /// Lists the binaries in the hoist toml registry.
    #[instrument]
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
            HoistRegistry::print_color(&format!("{}: ", binary.name), Color::Blue, false)?;
            HoistRegistry::print_color(&binary.location.display().to_string(), Color::Cyan, true)?;
        }

        Ok(())
    }

    /// Prints text to stdout in the provided color.
    #[instrument]
    pub fn print_color(text: &str, color: Color, newline: bool) -> Result<()> {
        let mut stdout = StandardStream::stdout(ColorChoice::Always);
        stdout.set_color(ColorSpec::new().set_fg(Some(color)))?;
        let newline = if newline { "\n" } else { "" };
        write!(&mut stdout, "{}{}", text, newline)?;
        Ok(())
    }

    /// Hoists binaries from the hoist toml registry into scope.
    #[instrument(skip(binaries))]
    pub fn hoist(binaries: Vec<String>) -> Result<()> {
        HoistRegistry::setup()?;

        // Then we read the registry file into a HoistRegistry object.
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new().read(true).open(registry_file)?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let registry: HoistRegistry = toml::from_str(&registry_toml)?;

        if !registry.binaries.iter().any(|b| binaries.contains(&b.name)) {
            anyhow::bail!("Failed to find binaries in hoist registry");
        }

        tracing::debug!("Hoisting {} binaries", std::env::current_dir()?.display());
        registry
            .binaries
            .iter()
            .filter(|b| binaries.contains(&b.name))
            .try_for_each(|b| b.copy_to_current_dir())?;

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
        // default to bash for now
        Ok(ShellType::Bash)
        // Err(anyhow::anyhow!("Unable to determine the user's shell."))
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
    use std::os::unix::prelude::OpenOptionsExt;

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
