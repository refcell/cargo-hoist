//! Shell Utilities

use anyhow::Result;
use std::path::PathBuf;

/// The bash function to install the hoist cargo pre-hook.
pub const INSTALL_BASH_FUNCTION: &str = r#"
function cargo() {
    if ~/.cargo/bin/cargo hoist --help &>/dev/null; then
      ~/.cargo/bin/cargo hoist install
    fi
    ~/.cargo/bin/cargo "$@"
}
"#;

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
    }
}
