//! Hoisted Binaries
//!
//! Core logic for working with hoisted binaries.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::instrument;

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
    pub fn new(name: impl Into<String>, location: PathBuf) -> Self {
        Self {
            name: name.into(),
            location,
        }
    }

    /// Copies the binary to the specified directory, [`dir`].
    #[instrument]
    pub fn copy_to_dir(&self, dir: &Path) -> Result<()> {
        let binary_path = dir.join(&self.name);
        tracing::debug!("Copying binary to current directory: {:?}", binary_path);
        std::fs::copy(&self.location, binary_path)?;
        Ok(())
    }

    /// Copies the binary to the current directory.
    #[instrument]
    pub fn copy_to_current_dir(&self) -> Result<()> {
        let current_dir = std::env::current_dir()?;
        self.copy_to_dir(&current_dir)
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
        test_dir
    }

    fn create_binaries(p: &Path) -> Vec<HoistedBinary> {
        let target_dir = p.join("target/release/");
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
        res.push(HoistedBinary::new("binary1", bin1_path));
        res.push(HoistedBinary::new("binary2", bin2_path));
        res
    }

    #[test]
    #[serial]
    fn test_copy_to_dir() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_dir = setup_test(&tempdir, "test_copy_to_dir");
        let dest = std::env::current_dir().unwrap().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let bins = create_binaries(&test_dir);
        for b in bins {
            assert!(!dest.join(&b.name).exists());
            b.copy_to_dir(&dest).unwrap();
            assert!(dest.join(&b.name).exists());
        }
    }

    #[test]
    #[serial]
    fn test_copy_to_current_dir() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_dir = setup_test(&tempdir, "test_copy_to_current_dir");
        let bins = create_binaries(&test_dir);
        for b in bins {
            assert!(!test_dir.join(&b.name).exists());
            b.copy_to_current_dir().unwrap();
            assert!(test_dir.join(&b.name).exists());
        }
    }
}
