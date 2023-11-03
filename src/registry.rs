//! Hoist Registry
//!
//! The registry module contains the core hoist registry logic.

use anyhow::Result;
use inquire::Confirm;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use tracing::instrument;

use crate::binaries::HoistedBinary;
use crate::shell::*;

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
    pub fn hoist(mut binaries: Vec<String>) -> Result<()> {
        HoistRegistry::setup()?;

        // Then we read the registry file into a HoistRegistry object.
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new().read(true).open(registry_file)?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let registry: HoistRegistry = toml::from_str(&registry_toml)?;

        // If binaries not contained in the global registry,
        // check the local build path to see if we want to hoist a local
        // bin.
        let mut registered = registry.binaries;
        if !registered.iter().any(|b| binaries.contains(&b.name)) {
            // todo(refcell): fuzzy match binaries in case of mispellings
            //                if found, prompt the user with an inquire confirm
            let local_bins = HoistRegistry::grab_binaries()
                .map_err(|_| anyhow::anyhow!("no global or local binaries match"))?;
            binaries = local_bins.clone();
            let target_dir = std::env::current_dir()?.join("target/release/");
            local_bins
                .into_iter()
                .map(|b| HoistedBinary::new(b.to_owned(), target_dir.join(b).clone()))
                .for_each(|hb| {
                    let _ = registered.insert(hb);
                });
        }

        tracing::debug!("Hoisting {} binaries", binaries.len());
        tracing::debug!("Hoist dest: {}", std::env::current_dir()?.display());
        registered
            .iter()
            .filter(|b| binaries.contains(&b.name))
            .try_for_each(|b| match b.copy_to_current_dir() {
                Ok(_) => {
                    HoistRegistry::print_color("Successfully hoisted ", Color::Green, false)?;
                    HoistRegistry::print_color(&b.name, Color::Magenta, true)?;
                    Ok(())
                }
                Err(e) => Err(e),
            })?;

        Ok(())
    }
}
