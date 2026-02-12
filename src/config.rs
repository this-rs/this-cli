use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Root configuration for a this-rs workspace, stored in `this.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceConfig {
    /// Workspace name (used for display and identifiers)
    pub name: String,

    /// API (backend) configuration
    pub api: ApiConfig,

    /// Deployment targets (webapp, desktop, ios, android)
    #[serde(default)]
    pub targets: Vec<TargetConfig>,
}

/// Configuration for the API (this-rs backend) within a workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiConfig {
    /// Relative path to the API directory (default: "api")
    #[serde(default = "default_api_path")]
    pub path: String,

    /// Default server port
    #[serde(default = "default_api_port")]
    pub port: u16,
}

/// Configuration for a deployment target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TargetConfig {
    /// Target type
    pub target_type: TargetType,

    /// Frontend framework (for webapp targets)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,

    /// Runtime (e.g., "tauri", "capacitor")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,

    /// Relative path to the target directory
    pub path: String,
}

/// Supported target types for a workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TargetType {
    Webapp,
    Website,
    Desktop,
    Ios,
    Android,
}

fn default_api_path() -> String {
    "api".to_string()
}

fn default_api_port() -> u16 {
    3000
}

/// Load a workspace configuration from a `this.yaml` file.
pub fn load_workspace_config(path: &Path) -> Result<WorkspaceConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read workspace config: {}", path.display()))?;
    let config: WorkspaceConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse workspace config: {}", path.display()))?;
    Ok(config)
}

/// Save a workspace configuration to a `this.yaml` file.
#[allow(dead_code)] // Will be used by `this init --workspace` (Task 2)
pub fn save_workspace_config(path: &Path, config: &WorkspaceConfig) -> Result<()> {
    let content =
        serde_yaml::to_string(config).with_context(|| "Failed to serialize workspace config")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write workspace config: {}", path.display()))?;
    Ok(())
}

impl std::fmt::Display for TargetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TargetType::Webapp => write!(f, "webapp"),
            TargetType::Website => write!(f, "website"),
            TargetType::Desktop => write!(f, "desktop"),
            TargetType::Ios => write!(f, "ios"),
            TargetType::Android => write!(f, "android"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_config() -> WorkspaceConfig {
        WorkspaceConfig {
            name: "my-project".to_string(),
            api: ApiConfig {
                path: "api".to_string(),
                port: 3000,
            },
            targets: vec![TargetConfig {
                target_type: TargetType::Webapp,
                framework: Some("react".to_string()),
                runtime: None,
                path: "front".to_string(),
            }],
        }
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let config = make_test_config();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: WorkspaceConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn test_load_save_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("this.yaml");
        let config = make_test_config();

        save_workspace_config(&path, &config).unwrap();
        let loaded = load_workspace_config(&path).unwrap();

        assert_eq!(config, loaded);
    }

    #[test]
    fn test_deserialize_minimal_config() {
        let yaml = r#"
name: test-project
api:
  path: api
  port: 8080
"#;
        let config: WorkspaceConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, "test-project");
        assert_eq!(config.api.port, 8080);
        assert!(config.targets.is_empty());
    }

    #[test]
    fn test_deserialize_with_defaults() {
        let yaml = r#"
name: test-project
api: {}
"#;
        let config: WorkspaceConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.api.path, "api");
        assert_eq!(config.api.port, 3000);
    }

    #[test]
    fn test_deserialize_full_config() {
        let yaml = r#"
name: my-app
api:
  path: api
  port: 3000
targets:
  - target_type: webapp
    framework: react
    path: front
  - target_type: desktop
    runtime: tauri
    path: targets/desktop
  - target_type: ios
    runtime: capacitor
    path: targets/ios
"#;
        let config: WorkspaceConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.name, "my-app");
        assert_eq!(config.targets.len(), 3);
        assert_eq!(config.targets[0].target_type, TargetType::Webapp);
        assert_eq!(config.targets[0].framework, Some("react".to_string()));
        assert_eq!(config.targets[1].target_type, TargetType::Desktop);
        assert_eq!(config.targets[1].runtime, Some("tauri".to_string()));
        assert_eq!(config.targets[2].target_type, TargetType::Ios);
    }

    #[test]
    fn test_load_invalid_yaml_returns_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("this.yaml");
        std::fs::write(&path, "not: valid: yaml: {{{}}}").unwrap();
        let result = load_workspace_config(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_missing_file_returns_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nonexistent.yaml");
        let result = load_workspace_config(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_target_type_display() {
        assert_eq!(TargetType::Webapp.to_string(), "webapp");
        assert_eq!(TargetType::Desktop.to_string(), "desktop");
        assert_eq!(TargetType::Ios.to_string(), "ios");
        assert_eq!(TargetType::Android.to_string(), "android");
        assert_eq!(TargetType::Website.to_string(), "website");
    }
}
