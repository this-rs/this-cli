use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::config;

/// Detect the root of a this-rs project by walking up from the current directory.
/// A this-rs project is identified by a Cargo.toml that contains a dependency on `this`.
///
/// In a workspace context, if a `this.yaml` is found first, the function resolves the API
/// directory from the workspace config (typically `api/`) and returns that path.
/// This allows commands like `this add entity` to work from the workspace root.
pub fn detect_project_root() -> Result<PathBuf> {
    let mut current = std::env::current_dir()?;

    loop {
        // Check for a direct this-rs project (Cargo.toml with `this` dependency)
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = std::fs::read_to_string(&cargo_toml)?;
            if content.contains("[dependencies]") && content.contains("this") {
                return Ok(current);
            }
        }

        // Check for a workspace root (this.yaml) and resolve to the API directory
        let this_yaml = current.join("this.yaml");
        if this_yaml.exists()
            && let Ok(workspace_config) = config::load_workspace_config(&this_yaml)
        {
            let api_dir = current.join(&workspace_config.api.path);
            let api_cargo = api_dir.join("Cargo.toml");
            if api_cargo.exists() {
                let content = std::fs::read_to_string(&api_cargo)?;
                if content.contains("[dependencies]") && content.contains("this") {
                    return Ok(api_dir);
                }
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

/// Find the workspace root by walking up from the current directory, looking for `this.yaml`.
/// Returns `None` if not inside a workspace.
#[allow(dead_code)] // Will be used by `this info` workspace display (Task 3)
pub fn find_workspace_root() -> Option<PathBuf> {
    find_workspace_root_from(&std::env::current_dir().ok()?)
}

/// Find the workspace root starting from a given path.
/// Useful for testing without changing the current directory.
#[allow(dead_code)] // Will be used by `this info` workspace display (Task 3)
pub fn find_workspace_root_from(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();

    loop {
        let this_yaml = current.join("this.yaml");
        if this_yaml.exists() {
            return Some(current);
        }

        if !current.pop() {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_find_workspace_root_from_root() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        // Create this.yaml at root
        std::fs::write(
            root.join("this.yaml"),
            "name: test\napi:\n  path: api\n  port: 3000\n",
        )
        .unwrap();

        let result = find_workspace_root_from(&root);
        assert_eq!(result, Some(root));
    }

    #[test]
    fn test_find_workspace_root_from_subdirectory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        // Create this.yaml at root
        std::fs::write(
            root.join("this.yaml"),
            "name: test\napi:\n  path: api\n  port: 3000\n",
        )
        .unwrap();

        // Create subdirectory api/src
        let sub = root.join("api").join("src");
        std::fs::create_dir_all(&sub).unwrap();

        let result = find_workspace_root_from(&sub);
        assert_eq!(result, Some(root));
    }

    #[test]
    fn test_find_workspace_root_returns_none_outside_workspace() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        // No this.yaml anywhere
        let result = find_workspace_root_from(&root);
        assert!(result.is_none());
    }
}
