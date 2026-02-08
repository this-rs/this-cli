use std::path::PathBuf;

use anyhow::{Result, bail};

/// Detect the root of a this-rs project by walking up from the current directory.
/// A this-rs project is identified by a Cargo.toml that contains a dependency on `this`.
pub fn detect_project_root() -> Result<PathBuf> {
    let mut current = std::env::current_dir()?;

    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = std::fs::read_to_string(&cargo_toml)?;
            // Check if this is a this-rs project (has `this` dependency)
            if content.contains("[dependencies]") && content.contains("this") {
                return Ok(current);
            }
        }

        if !current.pop() {
            break;
        }
    }

    bail!(
        "Not inside a this-rs project. Could not find a Cargo.toml with a 'this' dependency.\n\
         Run 'this init <name>' to create a new project, or navigate to an existing one."
    )
}
