//! Project
//!
//! The [Project] is a wrapper for interacting with rust projects and their output binaries.

use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::instrument;

use crate::binaries::HoistedBinary;

/// Project
#[derive(Debug, Default, Clone, Hash, Eq, PartialEq)]
pub struct Project {
    /// The project root location
    pub root: PathBuf,
    /// Binaries
    pub binaries: Vec<PathBuf>,
}

impl TryFrom<Option<&Path>> for Project {
    type Error = anyhow::Error;

    #[instrument(skip(p))]
    fn try_from(p: Option<&Path>) -> Result<Self> {
        let p = p
            .map(|p| p.to_path_buf())
            .unwrap_or(std::env::current_dir()?);
        Ok(Self {
            root: p.to_path_buf(),
            binaries: vec![],
        })
    }
}

impl TryFrom<Option<PathBuf>> for Project {
    type Error = anyhow::Error;

    #[instrument(skip(p))]
    fn try_from(p: Option<PathBuf>) -> Result<Self> {
        let p = p
            .map(|p| p.to_path_buf())
            .unwrap_or(std::env::current_dir()?);
        Ok(Self {
            root: p,
            binaries: vec![],
        })
    }
}

impl From<&Path> for Project {
    #[instrument(skip(p))]
    fn from(p: &Path) -> Self {
        Self {
            root: p.to_path_buf(),
            binaries: vec![],
        }
    }
}

impl Project {
    /// Create a new [Project] from the current dir.
    #[instrument]
    pub fn from_current_dir() -> Result<Self> {
        let root = std::env::current_dir()?;
        Ok(Self {
            root,
            binaries: vec![],
        })
    }

    /// Constructs project binaries from a list of binary string names.
    #[instrument(skip(self))]
    pub fn set_binaries(&mut self, binaries: Vec<String>) -> Result<()> {
        self.load()?;
        let mut bins = vec![];
        for binary in binaries {
            // Try to find the binary in the project target directories.
            let binary = self
                .binaries
                .iter()
                .find(|b| b.file_name().unwrap_or_default().to_string_lossy() == binary)
                .cloned();
            bins.push(binary.ok_or(anyhow::anyhow!("[std] failed to find binary"))?);
        }
        self.binaries = bins;
        Ok(())
    }

    /// Builds [HoistedBinary] objects from the project binaries.
    #[instrument(skip(self))]
    pub fn hoisted_binaries(&mut self) -> Result<Vec<HoistedBinary>> {
        let mut hoisted = vec![];
        for binary in &self.binaries {
            let binary_name = binary
                .file_name()
                .ok_or(anyhow::anyhow!("[std] failed to extract binary name"))?;
            let binary_name = binary_name
                .to_str()
                .ok_or(anyhow::anyhow!(
                    "[std] failed to convert binary path name to string"
                ))?
                .to_string();
            let binary = HoistedBinary::new(binary_name, binary.clone());
            hoisted.push(binary);
        }
        Ok(hoisted)
    }

    /// Get a list of targets for the project.
    #[instrument(skip(self))]
    pub fn get_targets(&self) -> Result<Vec<String>> {
        let mut targets = vec![];
        if !self.root.join("target").exists() {
            return Ok(targets);
        }
        for entry in std::fs::read_dir(self.root.join("target"))? {
            let Ok(e) = entry else {
                tracing::warn!("Failed to read entry: {:?}", entry);
                continue;
            };
            let Ok(target) = e.file_name().into_string() else {
                tracing::warn!("Failed to convert entry to string: {:?}", e);
                continue;
            };
            tracing::debug!("Found target: {}", target);
            targets.push(target);
        }
        tracing::debug!("Returning {} targets", targets.len());
        Ok(targets)
    }

    /// Attempts to load local binaries from the target directory.
    #[instrument(skip(self))]
    pub fn load(&mut self) -> Result<()> {
        let targets = self.get_targets()?;
        let mut binaries = vec![];
        for target in targets {
            let target = self.root.join("target").join(target);
            let bins = Project::extract_binaries(&target)?;
            binaries.extend(bins);
        }
        tracing::debug!("Returning {} binaries", binaries.len());
        self.binaries = binaries;
        Ok(())
    }

    /// Extract binaries from a target directory.
    #[instrument(skip(target))]
    pub fn extract_binaries(target: &Path) -> Result<Vec<PathBuf>> {
        let mut binaries = vec![];
        if !target.exists() {
            return Ok(binaries);
        }
        for entry in std::fs::read_dir(target)? {
            let Ok(e) = entry else {
                tracing::warn!("Failed to read entry: {:?}", entry);
                continue;
            };
            let Ok(exec) = crate::executables::exec_path(&e.path()) else {
                tracing::warn!("Failed to get exec path: {:?}", e);
                continue;
            };
            tracing::debug!("Found binary: {}", exec);
            let exec = std::fs::canonicalize(target.join(exec))?;
            binaries.push(exec);
        }
        tracing::debug!("Returning {} binaries", binaries.len());
        Ok(binaries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::os::unix::fs::OpenOptionsExt;
    use tempfile::TempDir;

    fn setup_test(tempdir: &TempDir, t: &str) -> PathBuf {
        let test_dir = tempdir.path().join(t);
        std::fs::create_dir(&test_dir).unwrap();
        std::env::set_current_dir(&test_dir).unwrap();
        test_dir
    }

    fn create_binaries(p: &Path) -> Vec<PathBuf> {
        let target_dir = p.join("target").join("release");
        std::fs::create_dir_all(&target_dir).unwrap();
        let bin1_path = target_dir.join("binary1");
        let bin2_path = target_dir.join("binary2");
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(&bin1_path)
            .unwrap();
        opts.sync_all().unwrap();
        let opts = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o755)
            .open(&bin2_path)
            .unwrap();
        opts.sync_all().unwrap();
        let mut res = Vec::with_capacity(2);
        res.push(std::fs::canonicalize(bin1_path).unwrap());
        res.push(std::fs::canonicalize(bin2_path).unwrap());
        res
    }

    #[test]
    #[serial]
    fn test_from_none_pathbuf() {
        let tempdir = tempfile::tempdir().unwrap();
        let _ = setup_test(&tempdir, "test_from_none_pathbuf");
        let project = Project::try_from(None::<PathBuf>).unwrap();
        assert_eq!(project.root, std::env::current_dir().unwrap());
    }

    #[test]
    #[serial]
    fn test_from_none_path() {
        let tempdir = tempfile::tempdir().unwrap();
        let _ = setup_test(&tempdir, "test_from_none_path");
        let project = Project::try_from(None::<&Path>).unwrap();
        assert_eq!(project.root, std::env::current_dir().unwrap());
    }

    #[test]
    #[serial]
    fn test_missing_target() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_dir = setup_test(&tempdir, "test_missing_target");
        let project = Project::from(test_dir.as_path());
        assert!(project.get_targets().unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn test_get_targets() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_dir = setup_test(&tempdir, "test_get_targets");
        let project = Project::from(test_dir.as_path());
        create_binaries(&test_dir);
        let targets = project.get_targets().unwrap();
        assert_eq!(targets, vec!["release"]);
    }

    #[test]
    #[serial]
    fn test_extract_missing_target() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_dir = setup_test(&tempdir, "test_extract_missing_target");
        let target = test_dir.join("target/release");
        assert!(Project::extract_binaries(&target).unwrap().is_empty());
    }

    #[test]
    #[serial]
    fn test_extract_binaries() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_dir = setup_test(&tempdir, "test_extract_binaries");
        let mut binaries = create_binaries(&test_dir);
        let target = test_dir.join("target").join("release");
        let mut extracted = Project::extract_binaries(&target).unwrap();
        extracted.sort();
        binaries.sort();
        assert_eq!(extracted, binaries);
    }
}
