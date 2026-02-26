//! Shared test infrastructure for this-cli unit tests.
//!
//! Provides factory functions to create scaffold projects/workspaces in temp dirs,
//! assertion helpers for file content verification, and common test utilities.
//!
//! Inspired by the `this` project's test harness pattern.

use std::path::Path;

use tempfile::TempDir;

// ============================================================================
// Factory functions
// ============================================================================

/// Create a minimal classic project scaffold for testing.
///
/// Creates a directory structure that mimics `this init <name>`:
/// ```text
/// <tmpdir>/<name>/
///   ├── Cargo.toml          (with this dependency)
///   ├── src/
///   │   ├── main.rs
///   │   └── domain/
///   │       ├── mod.rs
///   │       └── stores.rs
///   └── config/
///       └── links.yaml
/// ```
///
/// Returns the path to the project root.
pub fn setup_test_project(tmp: &TempDir, name: &str, backend: &str) -> std::path::PathBuf {
    let project = tmp.path().join(name);
    std::fs::create_dir_all(project.join("src/domain")).unwrap();
    std::fs::create_dir_all(project.join("config")).unwrap();

    // Cargo.toml with this dependency
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
this = "0.0.8"
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1"
uuid = {{ version = "1", features = ["v4"] }}
serde = {{ version = "1", features = ["derive"] }}
"#
    );
    std::fs::write(project.join("Cargo.toml"), cargo_toml).unwrap();

    // main.rs
    let main_rs = r#"use this::prelude::*;

mod domain;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = ServerHost::builder()
        .port(3000)
        .build()?;
    host.run().await
}
"#;
    std::fs::write(project.join("src/main.rs"), main_rs).unwrap();

    // domain/mod.rs
    std::fs::write(project.join("src/domain/mod.rs"), "").unwrap();

    // domain/stores.rs with backend-appropriate content
    let stores_content = match backend {
        "postgres" => {
            "use this::prelude::*;\n\npub fn register_stores(pool: &PgPool) {}\n".to_string()
        }
        _ => "use this::prelude::*;\n\npub fn register_stores() {}\n".to_string(),
    };
    std::fs::write(project.join("src/domain/stores.rs"), stores_content).unwrap();

    // config/links.yaml (empty)
    std::fs::write(
        project.join("config/links.yaml"),
        "entities: {}\nlinks: []\n",
    )
    .unwrap();

    project
}

/// Create a minimal workspace scaffold for testing.
///
/// Creates a directory structure that mimics `this init <name> --workspace`:
/// ```text
/// <tmpdir>/<name>/
///   ├── this.yaml
///   ├── Cargo.toml           (workspace)
///   └── api/
///       ├── Cargo.toml       (with this dependency)
///       ├── src/
///       │   ├── main.rs
///       │   └── domain/
///       │       ├── mod.rs
///       │       └── stores.rs
///       └── config/
///           └── links.yaml
/// ```
///
/// Returns the path to the workspace root.
pub fn setup_test_workspace(tmp: &TempDir, name: &str) -> std::path::PathBuf {
    let ws = tmp.path().join(name);
    std::fs::create_dir_all(ws.join("api/src/domain")).unwrap();
    std::fs::create_dir_all(ws.join("api/config")).unwrap();

    // this.yaml
    let this_yaml = format!("name: {name}\napi:\n  path: api\n  port: 3000\ntargets: []\n");
    std::fs::write(ws.join("this.yaml"), this_yaml).unwrap();

    // Workspace Cargo.toml
    let workspace_cargo = r#"[workspace]
members = ["api"]
resolver = "2"
"#;
    std::fs::write(ws.join("Cargo.toml"), workspace_cargo).unwrap();

    // API Cargo.toml
    let api_cargo = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
this = "0.0.8"
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1"
uuid = {{ version = "1", features = ["v4"] }}
serde = {{ version = "1", features = ["derive"] }}
"#
    );
    std::fs::write(ws.join("api/Cargo.toml"), api_cargo).unwrap();

    // API main.rs
    let main_rs = r#"use this::prelude::*;

mod domain;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = ServerHost::builder()
        .port(3000)
        .build()?;
    host.run().await
}
"#;
    std::fs::write(ws.join("api/src/main.rs"), main_rs).unwrap();

    // domain/mod.rs + stores.rs
    std::fs::write(ws.join("api/src/domain/mod.rs"), "").unwrap();
    std::fs::write(
        ws.join("api/src/domain/stores.rs"),
        "use this::prelude::*;\n\npub fn register_stores() {}\n",
    )
    .unwrap();

    // config/links.yaml
    std::fs::write(
        ws.join("api/config/links.yaml"),
        "entities: {}\nlinks: []\n",
    )
    .unwrap();

    ws
}

/// Create a workspace scaffold with a webapp target already added.
///
/// Same as `setup_test_workspace` but also adds:
/// - `front/` directory with a basic structure
/// - Webapp target in this.yaml
pub fn setup_test_workspace_with_webapp(tmp: &TempDir, name: &str) -> std::path::PathBuf {
    let ws = setup_test_workspace(tmp, name);

    // Create front directory
    std::fs::create_dir_all(ws.join("front/src")).unwrap();
    std::fs::write(ws.join("front/package.json"), "{}").unwrap();

    // Update this.yaml to include webapp target
    let this_yaml = format!(
        "name: {name}\napi:\n  path: api\n  port: 3000\ntargets:\n  - target_type: webapp\n    framework: react\n    path: front\n"
    );
    std::fs::write(ws.join("this.yaml"), this_yaml).unwrap();

    ws
}

/// Create a workspace scaffold with websocket support.
#[allow(dead_code)]
pub fn setup_test_workspace_with_websocket(tmp: &TempDir, name: &str) -> std::path::PathBuf {
    let ws = setup_test_workspace(tmp, name);

    // Update Cargo.toml with websocket feature
    let api_cargo = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
this = {{ version = "0.0.8", features = ["websocket"] }}
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1"
"#
    );
    std::fs::write(ws.join("api/Cargo.toml"), api_cargo).unwrap();

    // Update main.rs with WebSocketExposure
    let main_rs = r#"use this::prelude::*;

mod domain;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = ServerHost::builder()
        .port(3000)
        .add_exposure(WebSocketExposure)
        .build()?;
    host.run().await
}
"#;
    std::fs::write(ws.join("api/src/main.rs"), main_rs).unwrap();

    ws
}

/// Create a workspace scaffold with gRPC support.
#[allow(dead_code)]
pub fn setup_test_workspace_with_grpc(tmp: &TempDir, name: &str) -> std::path::PathBuf {
    let ws = setup_test_workspace(tmp, name);

    // Update Cargo.toml with grpc feature
    let api_cargo = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
this = {{ version = "0.0.8", features = ["grpc"] }}
tokio = {{ version = "1", features = ["full"] }}
anyhow = "1"
"#
    );
    std::fs::write(ws.join("api/Cargo.toml"), api_cargo).unwrap();

    // Update main.rs with GrpcExposure
    let main_rs = r#"use this::prelude::*;

mod domain;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = ServerHost::builder()
        .port(3000)
        .add_exposure(GrpcExposure::new(50051))
        .build()?;
    host.run().await
}
"#;
    std::fs::write(ws.join("api/src/main.rs"), main_rs).unwrap();

    ws
}

// ============================================================================
// Entity helpers — add an entity scaffold to an existing project
// ============================================================================

/// Add an entity scaffold to an existing project (classic or workspace API dir).
///
/// Creates the entity files that `this add entity` would generate:
/// - `src/domain/<name>/mod.rs`
/// - `src/domain/<name>/model.rs`
/// - `src/domain/<name>/store.rs`
/// - `src/domain/<name>/handlers.rs`
/// - `src/domain/<name>/descriptor.rs`
///
/// Also updates `src/domain/mod.rs` with `pub mod <name>;`.
pub fn add_entity_to_project(project_src: &Path, entity_name: &str) {
    let domain_dir = project_src.join("domain").join(entity_name);
    std::fs::create_dir_all(&domain_dir).unwrap();

    let pascal = to_pascal_case(entity_name);

    // model.rs
    let model = format!(
        r#"use serde::{{Deserialize, Serialize}};
use this::prelude::*;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct {pascal} {{
    pub id: Uuid,
    pub entity_type: String,
    pub name: String,
}}

impl_data_entity!({pascal}, "{entity_name}");
"#
    );
    std::fs::write(domain_dir.join("model.rs"), model).unwrap();

    // store.rs
    let store = format!(
        r#"use this::prelude::*;
use super::model::{pascal};

pub fn create_store() -> InMemoryDataService<{pascal}> {{
    InMemoryDataService::new()
}}
"#
    );
    std::fs::write(domain_dir.join("store.rs"), store).unwrap();

    // handlers.rs
    let handlers = format!(
        r#"use this::prelude::*;
use super::model::{pascal};
"#
    );
    std::fs::write(domain_dir.join("handlers.rs"), handlers).unwrap();

    // descriptor.rs
    let descriptor = format!(
        r#"use this::prelude::*;
use super::model::{pascal};

pub struct {pascal}Descriptor;

impl EntityDescriptor for {pascal}Descriptor {{
    fn entity_type(&self) -> &str {{ "{entity_name}" }}
    fn plural(&self) -> &str {{ "{entity_name}s" }}
    fn build_routes(&self) -> axum::Router {{
        axum::Router::new()
    }}
}}
"#
    );
    std::fs::write(domain_dir.join("descriptor.rs"), descriptor).unwrap();

    // mod.rs
    std::fs::write(
        domain_dir.join("mod.rs"),
        "pub mod model;\npub mod store;\npub mod handlers;\npub mod descriptor;\n",
    )
    .unwrap();

    // Update parent mod.rs
    let parent_mod = project_src.join("domain/mod.rs");
    let existing = std::fs::read_to_string(&parent_mod).unwrap_or_default();
    let mod_line = format!("pub mod {};", entity_name);
    if !existing.contains(&mod_line) {
        let updated = if existing.is_empty() {
            format!("{}\n", mod_line)
        } else {
            format!("{}\n{}\n", existing.trim_end(), mod_line)
        };
        std::fs::write(&parent_mod, updated).unwrap();
    }
}

// ============================================================================
// Assertion helpers
// ============================================================================

/// Assert that a file exists at the given relative path within a directory.
///
/// # Panics
/// Panics with a descriptive message if the file does not exist.
pub fn assert_file_exists(dir: &Path, relative: &str) {
    let path = dir.join(relative);
    assert!(
        path.exists(),
        "Expected file to exist: {} (full path: {})",
        relative,
        path.display()
    );
}

/// Assert that a file does NOT exist at the given relative path.
pub fn assert_file_not_exists(dir: &Path, relative: &str) {
    let path = dir.join(relative);
    assert!(
        !path.exists(),
        "Expected file to NOT exist: {} (full path: {})",
        relative,
        path.display()
    );
}

/// Assert that a directory exists at the given relative path.
pub fn assert_dir_exists(dir: &Path, relative: &str) {
    let path = dir.join(relative);
    assert!(
        path.is_dir(),
        "Expected directory to exist: {} (full path: {})",
        relative,
        path.display()
    );
}

/// Assert that a file contains the expected string.
///
/// # Panics
/// Panics if the file doesn't exist or doesn't contain the expected string.
pub fn assert_file_contains(dir: &Path, relative: &str, expected: &str) {
    let path = dir.join(relative);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read file {}: {}", path.display(), e));
    assert!(
        content.contains(expected),
        "File '{}' should contain '{}'\n--- Actual content ---\n{}",
        relative,
        expected,
        content
    );
}

/// Assert that a file does NOT contain the given string.
pub fn assert_file_not_contains(dir: &Path, relative: &str, unexpected: &str) {
    let path = dir.join(relative);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read file {}: {}", path.display(), e));
    assert!(
        !content.contains(unexpected),
        "File '{}' should NOT contain '{}'\n--- Actual content ---\n{}",
        relative,
        unexpected,
        content
    );
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Simple PascalCase conversion (for test data only).
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_test_project_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let project = setup_test_project(&tmp, "test_proj", "in-memory");

        assert_file_exists(&project, "Cargo.toml");
        assert_file_exists(&project, "src/main.rs");
        assert_file_exists(&project, "src/domain/mod.rs");
        assert_file_exists(&project, "src/domain/stores.rs");
        assert_file_exists(&project, "config/links.yaml");
        assert_file_contains(&project, "Cargo.toml", "this");
    }

    #[test]
    fn test_setup_test_workspace_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_test_workspace(&tmp, "test_ws");

        assert_file_exists(&ws, "this.yaml");
        assert_file_exists(&ws, "Cargo.toml");
        assert_file_exists(&ws, "api/Cargo.toml");
        assert_file_exists(&ws, "api/src/main.rs");
        assert_file_exists(&ws, "api/src/domain/mod.rs");
        assert_file_contains(&ws, "this.yaml", "name: test_ws");
        assert_file_contains(&ws, "Cargo.toml", "[workspace]");
    }

    #[test]
    fn test_setup_test_workspace_with_webapp() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_test_workspace_with_webapp(&tmp, "ws_webapp");

        assert_dir_exists(&ws, "front/src");
        assert_file_contains(&ws, "this.yaml", "target_type: webapp");
        assert_file_contains(&ws, "this.yaml", "framework: react");
    }

    #[test]
    fn test_add_entity_to_project() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_test_workspace(&tmp, "entity_test");

        add_entity_to_project(&ws.join("api/src"), "product");

        assert_file_exists(&ws, "api/src/domain/product/mod.rs");
        assert_file_exists(&ws, "api/src/domain/product/model.rs");
        assert_file_exists(&ws, "api/src/domain/product/store.rs");
        assert_file_contains(&ws, "api/src/domain/product/model.rs", "Product");
        assert_file_contains(&ws, "api/src/domain/mod.rs", "pub mod product;");
    }

    #[test]
    fn test_assert_file_not_exists() {
        let tmp = TempDir::new().unwrap();
        assert_file_not_exists(tmp.path(), "nonexistent.txt");
    }

    #[test]
    fn test_assert_file_not_contains() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("test.txt"), "hello world").unwrap();
        assert_file_not_contains(tmp.path(), "test.txt", "goodbye");
    }
}
