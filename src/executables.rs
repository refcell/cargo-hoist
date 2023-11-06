//! Executables
//!
//! Utilities for working with executables.

use anyhow::Result;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tracing::instrument;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn test_exec_path() {
        let tempdir = tempfile::tempdir().unwrap();
        let test_dir = setup_test(&tempdir, "test_exec_path");
        let binaries = create_binaries(&test_dir);
        let bin1 = binaries.get(0).unwrap();
        let bin1_path = test_dir.join("target/release/binary1");
        let bin1_exec_path = exec_path(&bin1_path).unwrap();
        assert_eq!(bin1, &bin1_exec_path);
    }

    fn setup_test(tempdir: &TempDir, t: &str) -> PathBuf {
        let test_dir = tempdir.path().join(t);
        std::fs::create_dir(&test_dir).unwrap();
        std::env::set_current_dir(&test_dir).unwrap();
        test_dir
    }

    fn create_binaries(p: &Path) -> Vec<String> {
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
        res.push("binary1".to_string());
        res.push("binary2".to_string());
        res
    }
}
