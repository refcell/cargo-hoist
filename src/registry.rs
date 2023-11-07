//! Hoist Registry
//!
//! The registry module contains the core hoist registry logic.

use anyhow::Result;
use inquire::Confirm;
use inquire::{
    formatter::MultiOptionFormatter, list_option::ListOption, validator::Validation, MultiSelect,
};
use is_terminal::IsTerminal;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
    pub fn create_dir(quiet: bool) -> Result<()> {
        let hoist_dir = HoistRegistry::dir()?;
        if !std::path::Path::new(&hoist_dir).exists() {
            if !quiet {
                tracing::info!("Creating ~/.hoist/ directory");
            }
            std::fs::create_dir(&hoist_dir)?;
        }
        Ok(())
    }

    /// Create the hoist registry file.
    pub fn create_registry(quiet: bool) -> Result<()> {
        HoistRegistry::create_dir(quiet)?;
        let registry_file = HoistRegistry::path()?;
        if !std::path::Path::new(&registry_file).exists() {
            HoistRegistry::default().write()?;
        }
        Ok(())
    }

    /// Build a new [HoistRegistry] from the registry file.
    pub fn new() -> Result<HoistRegistry> {
        let registry_file = HoistRegistry::path()?;
        let mut file = std::fs::OpenOptions::new().read(true).open(registry_file)?;
        file.sync_all()?;
        let mut registry_toml = String::new();
        file.read_to_string(&mut registry_toml)?;
        let registry: HoistRegistry = toml::from_str(&registry_toml)?;
        Ok(registry)
    }

    /// Create the hoist pre-hook in the user bash file.
    pub fn create_pre_hook(with_confirm: bool, quiet: bool) -> Result<()> {
        HoistRegistry::create_dir(quiet)?;
        let hook_file = HoistRegistry::hook_identifier()?;
        if !std::path::Path::new(&hook_file).exists() {
            let should_prompt = std::io::stdout().is_terminal() && with_confirm;
            if should_prompt {
                tracing::debug!("detected tty, prompting user for install");
            }
            if should_prompt && !Confirm::new("Cargo hoist pre-cargo hook not installed. Do you want to install? ([y]/n) Once installed, this prompt will not bother you again :)").prompt()? {
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
    pub fn setup(quiet: bool) -> Result<()> {
        HoistRegistry::create_dir(quiet)?;
        HoistRegistry::create_registry(quiet)?;
        HoistRegistry::create_pre_hook(false, quiet)?;
        Ok(())
    }

    /// Nukes the hoist toml registry.
    /// This writes an empty registry to the registry file.
    #[instrument]
    pub fn nuke(quiet: bool) -> Result<()> {
        HoistRegistry::setup(quiet)?;
        HoistRegistry::default().write()?;
        Ok(())
    }

    /// Installs binaries in the hoist toml registry.
    #[instrument(skip(pdir, binaries, quiet))]
    pub fn install(pdir: Option<&Path>, binaries: Vec<String>, quiet: bool) -> Result<()> {
        HoistRegistry::setup(quiet)?;

        // Build the hoist registry.
        let mut registry = HoistRegistry::new()?;

        // Load binaries from the project
        let mut p = match crate::project::Project::try_from(pdir) {
            Ok(p) => p,
            Err(e) => {
                if !quiet {
                    tracing::warn!("Failed to load project: {}", e);
                }
                return Ok(());
            }
        };
        let hoisted = if binaries.is_empty() {
            p.load()?;
            p.hoisted_binaries()?
        } else {
            p.set_binaries(binaries)?;
            p.hoisted_binaries()?
        };

        // Insert hoisted binaries
        let registered = hoisted.len();
        hoisted.into_iter().for_each(|hb| {
            registry.insert(hb);
        });

        // Only perform a writeback if there are binaries to hoist.
        match registered {
            0 => tracing::warn!("No binaries found in the target directory"),
            _ => registry.write()?,
        }

        Ok(())
    }

    /// Writes the [HoistRegistry] to the registry file.
    #[instrument(skip(self))]
    pub fn write(&self) -> Result<()> {
        let registry_file = HoistRegistry::path()?;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(registry_file)?;
        let toml = toml::to_string(&self)?;
        f.write_all(toml.as_bytes())?;
        f.sync_all()?;
        Ok(())
    }

    /// Finds a given binary in the hoist registry toml.
    #[instrument(skip(binary))]
    pub fn find(binary: impl AsRef<str>) -> Result<()> {
        HoistRegistry::setup(false)?;
        let registry = HoistRegistry::new()?;

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
    pub fn list(quiet: bool) -> Result<()> {
        HoistRegistry::setup(quiet)?;
        let registry = HoistRegistry::new()?;
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
    pub fn hoist(binaries: Vec<String>, quiet: bool) -> Result<()> {
        HoistRegistry::setup(quiet)?;
        let registry = HoistRegistry::new()?;

        // If binaries not contained in the global registry,
        // check the local build path to see if we want to hoist a local
        // bin.
        let mut registered = registry.binaries;
        if !registered.iter().any(|b| binaries.contains(&b.name)) {
            // todo(refcell): fuzzy match binaries in case of mispellings
            //                if found, prompt the user with an inquire confirm
            let hoisted = crate::project::Project::from_current_dir()?.hoisted_binaries()?;
            hoisted.into_iter().for_each(|hb| {
                let _ = registered.insert(hb);
            });
        }

        // If the user provided no binaries, use an inquire select to prompt
        // the user to select which binaries to hoist.
        let mut selected;
        if binaries.is_empty() {
            selected = HoistRegistry::multiselect_registered(&registered, quiet)?;
        }
        // If no tty, hoist all binaries, including redundant ones.
        else if !std::io::stdout().is_terminal() {
            selected = registered
                .into_iter()
                .filter(|b| binaries.contains(&b.name))
                .collect();
        }
        // Otherwise, we want to convert the binaries to a set of de-duplicated hoisted binaries.
        else {
            let found: Vec<_> = registered
                .into_iter()
                .filter(|b| binaries.contains(&b.name))
                .collect();
            // Get the non-duplicate binaries from the found binaries.
            let non_duplicate = found
                .iter()
                .filter(|b| {
                    found
                        .iter()
                        .filter(|b2| b2.name == b.name)
                        .collect::<Vec<_>>()
                        .len()
                        == 1
                })
                .cloned()
                .collect::<Vec<_>>();

            HoistRegistry::print_color(
                &format!(
                    "Found {} conflicting registered binaries, opening a multiselect prompt to select which binaries to hoist.",
                    found.len()
                ),
                Color::Yellow,
                true,
            )?;
            selected = HoistRegistry::multiselect_registered(
                &HashSet::from_iter(found.into_iter()),
                quiet,
            )?;
            // Extend the selected binaries with the non-duplicate binaries.
            selected.extend(non_duplicate);
        }

        selected
            .iter()
            .try_for_each(|b| match b.copy_to_current_dir() {
                Ok(_) => {
                    if !quiet {
                        HoistRegistry::print_color("Successfully hoisted ", Color::Green, false)?;
                        HoistRegistry::print_color(&b.name, Color::Magenta, true)?;
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            })
    }

    /// Prompts the user for a list of hoisted binaries with a [MultiSelect].
    #[instrument(skip(registered, quiet))]
    pub fn multiselect_registered(
        registered: &HashSet<HoistedBinary>,
        quiet: bool,
    ) -> Result<Vec<HoistedBinary>> {
        let options = registered
            .iter()
            .map(|b| format!("{} ({})", b.name, b.location.display()))
            .collect();
        let validator = move |a: &[ListOption<&String>]| {
            if !quiet {
                tracing::debug!("Received binary input selection: {:?}", a);
            }
            Ok(Validation::Valid)
        };
        let formatter: MultiOptionFormatter<'_, String> =
            &|a| format!("{} different bins", a.len());
        match MultiSelect::new("Which binaries would you like to hoist?", options)
            .with_validator(validator)
            .with_formatter(formatter)
            .prompt()
        {
            Ok(choices) => {
                // The maximum hoisted binary size is how many binaries are registered.
                let mut res = Vec::with_capacity(choices.len());
                for c in choices {
                    let mut split = c.split_whitespace();
                    let name = split.next().ok_or(anyhow::anyhow!(
                        "Failed to parse selected binary name: {}",
                        c
                    ))?;
                    let location = split.next().ok_or(anyhow::anyhow!(
                        "Failed to parse selected binary location: {}",
                        c
                    ))?;
                    let location = location
                        .trim_start_matches('(')
                        .trim_end_matches(')')
                        .to_string();
                    res.push(HoistedBinary::new(
                        name.to_string(),
                        PathBuf::from(location),
                    ));
                }
                Ok(res)
            }
            Err(e) => {
                if !quiet {
                    HoistRegistry::print_color(
                        "Failed to hoist selected binary",
                        Color::Red,
                        true,
                    )?;
                }
                Err(e.into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::os::unix::prelude::OpenOptionsExt;
    use tempfile::TempDir;

    fn setup_test(tempdir: &TempDir, t: &str) -> PathBuf {
        let test_dir = tempdir.path().join(t);
        std::fs::create_dir(&test_dir).unwrap();
        std::env::set_current_dir(&test_dir).unwrap();

        let bash_file = test_dir.join(".bashrc");
        std::fs::File::create(&bash_file).unwrap();
        let zshrc = test_dir.join(".zshrc");
        std::fs::File::create(&zshrc).unwrap();

        let target_dir = test_dir.join("target/release/");
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

        std::env::set_var("HOME", &test_dir);

        test_dir
    }

    #[test]
    #[serial]
    fn test_setup() {
        let original_home = std::env::var_os("HOME").unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = setup_test(&tempdir, "test_setup");

        HoistRegistry::setup(false).unwrap();

        assert_eq!(HoistRegistry::new().unwrap(), HoistRegistry::default());

        let hook_file = HoistRegistry::hook_identifier().unwrap();
        assert!(std::path::Path::new(&hook_file).exists());
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .open(test_tempdir.join(".bashrc"))
            .unwrap();
        let mut bash_file_contents = String::new();
        file.read_to_string(&mut bash_file_contents).unwrap();

        // If the bash file is empty, try to read the zshrc file.
        if bash_file_contents.is_empty() {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .open(test_tempdir.join(".zshrc"))
                .unwrap();
            let mut zshrc_file_contents = String::new();
            file.read_to_string(&mut zshrc_file_contents).unwrap();
            assert_eq!(zshrc_file_contents, INSTALL_BASH_FUNCTION);
        } else {
            assert_eq!(bash_file_contents, INSTALL_BASH_FUNCTION);
        }

        std::env::set_current_dir(&original_home).unwrap();
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_install() {
        let original_home = std::env::var_os("HOME").unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = setup_test(&tempdir, "test_install");

        HoistRegistry::install(Some(&test_tempdir), Vec::new(), false).unwrap();

        assert_eq!(
            HoistRegistry::new().unwrap(),
            HoistRegistry {
                binaries: HashSet::from([
                    HoistedBinary::new(
                        "binary1".to_string(),
                        test_tempdir
                            .join("target/release/binary1")
                            .canonicalize()
                            .unwrap()
                    ),
                    HoistedBinary::new(
                        "binary2".to_string(),
                        test_tempdir
                            .join("target/release/binary2")
                            .canonicalize()
                            .unwrap()
                    ),
                ])
            }
        );

        std::env::set_current_dir(&original_home).unwrap();
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_multiple_installs() {
        let original_home = std::env::var_os("HOME").unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = setup_test(&tempdir, "test_multiple_installs");

        HoistRegistry::install(Some(&test_tempdir), Vec::new(), false).unwrap();
        HoistRegistry::install(Some(&test_tempdir), Vec::new(), false).unwrap();
        HoistRegistry::install(Some(&test_tempdir), Vec::new(), false).unwrap();
        HoistRegistry::install(Some(&test_tempdir), Vec::new(), false).unwrap();

        assert_eq!(
            HoistRegistry::new().unwrap(),
            HoistRegistry {
                binaries: HashSet::from([
                    HoistedBinary::new(
                        "binary1".to_string(),
                        test_tempdir
                            .join("target/release/binary1")
                            .canonicalize()
                            .unwrap()
                    ),
                    HoistedBinary::new(
                        "binary2".to_string(),
                        test_tempdir
                            .join("target/release/binary2")
                            .canonicalize()
                            .unwrap()
                    ),
                ])
            }
        );

        std::env::set_current_dir(&original_home).unwrap();
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_hoist() {
        let original_home = std::env::var_os("HOME").unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = setup_test(&tempdir, "test_hoist");

        HoistRegistry::install(Some(&test_tempdir), Vec::new(), false).unwrap();

        HoistRegistry::hoist(vec!["binary1".to_string()], false).unwrap();
        HoistRegistry::hoist(vec!["binary1".to_string()], false).unwrap();

        let binary1 = std::env::current_dir().unwrap().join("binary1");
        assert!(std::path::Path::new(&binary1).exists());
        let binary2 = std::env::current_dir().unwrap().join("binary2");
        assert!(!std::path::Path::new(&binary2).exists());

        std::env::set_current_dir(&original_home).unwrap();
        std::env::set_var("HOME", original_home);
    }

    #[test]
    #[serial]
    fn test_nuke() {
        let original_home = std::env::var_os("HOME").unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let test_tempdir = setup_test(&tempdir, "test_nuke");

        HoistRegistry::install(Some(&test_tempdir), Vec::new(), false).unwrap();

        HoistRegistry::nuke(false).unwrap();

        assert_eq!(HoistRegistry::new().unwrap(), HoistRegistry::default());

        std::env::set_current_dir(&original_home).unwrap();
        std::env::set_var("HOME", original_home);
    }
}
