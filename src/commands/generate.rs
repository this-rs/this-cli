//! `this generate client` — generate a typed API client from project introspection

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

use super::GenerateClientArgs;
use crate::codegen::introspect;
use crate::config::{TargetType, load_workspace_config};
use crate::utils::file_writer::FileWriter;
use crate::utils::{output, project};

pub fn run(args: GenerateClientArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
    run_in(args, writer, &cwd)
}

/// Run the generate client command with an explicit starting directory.
/// This avoids relying on the process-global CWD, making it safe for parallel tests.
pub(crate) fn run_in(args: GenerateClientArgs, writer: &dyn FileWriter, cwd: &Path) -> Result<()> {
    if args.lang != "typescript" {
        bail!(
            "Unsupported language: '{}'. Currently only 'typescript' is supported.",
            args.lang
        );
    }

    // Find workspace root and API directory
    let workspace_root = project::find_workspace_root_from(cwd).ok_or_else(|| {
        anyhow::anyhow!("Not inside a this-rs workspace. Run `this init <name> --workspace` first.")
    })?;

    let this_yaml = workspace_root.join("this.yaml");
    let config = load_workspace_config(&this_yaml)?;
    let api_root = workspace_root.join(&config.api.path);

    if writer.is_dry_run() {
        output::print_step("Dry run — no files will be written");
    }

    // Introspect the project
    output::print_step("Introspecting project entities and links...");
    let project = introspect::introspect(&api_root)?;

    if project.entities.is_empty() {
        bail!(
            "No entities found in {}. Add entities with `this add entity <name>` first.",
            api_root.join("src/entities").display()
        );
    }

    output::print_info(&format!(
        "Found {} entities, {} links",
        project.entities.len(),
        project.links.len()
    ));

    // Generate TypeScript client
    output::print_step("Generating TypeScript API client...");
    let ts_content = crate::codegen::typescript::generate(&project);

    // Determine output path
    let output_path = match args.output {
        Some(path) => path,
        None => auto_detect_output(&workspace_root, &config)?,
    };

    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        writer.create_dir_all(parent)?;
    }

    writer.write_file(&output_path, &ts_content)?;
    output::print_file_created(&output_path.display().to_string());

    output::print_success(&format!(
        "Generated API client: {} ({} entities, {} links)",
        output_path.display(),
        project.entities.len(),
        project.links.len()
    ));

    Ok(())
}

/// Auto-detect the output path from this.yaml webapp target.
/// Falls back to `<workspace>/api-client.ts` if no webapp target exists.
fn auto_detect_output(
    workspace_root: &std::path::Path,
    config: &crate::config::WorkspaceConfig,
) -> Result<PathBuf> {
    // Try to find webapp target
    if let Some(webapp) = config
        .targets
        .iter()
        .find(|t| t.target_type == TargetType::Webapp)
    {
        Ok(workspace_root.join(&webapp.path).join("src/api-client.ts"))
    } else {
        // No webapp target — output next to this.yaml
        Ok(workspace_root.join("api-client.ts"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::GenerateClientArgs;
    use tempfile::TempDir;

    #[test]
    fn test_auto_detect_output_with_webapp() {
        let root = PathBuf::from("/project");
        let config = crate::config::WorkspaceConfig {
            name: "test".to_string(),
            api: crate::config::ApiConfig {
                path: "api".to_string(),
                port: 3000,
            },
            targets: vec![crate::config::TargetConfig {
                target_type: TargetType::Webapp,
                framework: Some("react".to_string()),
                runtime: None,
                path: "front".to_string(),
            }],
        };
        let path = auto_detect_output(&root, &config).unwrap();
        assert_eq!(path, PathBuf::from("/project/front/src/api-client.ts"));
    }

    #[test]
    fn test_auto_detect_output_no_webapp() {
        let root = PathBuf::from("/project");
        let config = crate::config::WorkspaceConfig {
            name: "test".to_string(),
            api: crate::config::ApiConfig {
                path: "api".to_string(),
                port: 3000,
            },
            targets: vec![],
        };
        let path = auto_detect_output(&root, &config).unwrap();
        assert_eq!(path, PathBuf::from("/project/api-client.ts"));
    }

    // ── Helper to create a workspace with entities ───────────────────

    /// Create a workspace scaffold with this.yaml + api/ containing an entity.
    fn setup_generate_workspace(tmp: &TempDir, name: &str) -> std::path::PathBuf {
        let ws = tmp.path().join(name);
        std::fs::create_dir_all(ws.join("api/src/entities/product")).unwrap();
        std::fs::create_dir_all(ws.join("api/config")).unwrap();

        // this.yaml
        let yaml = format!(
            "name: {}\napi:\n  path: api\n  port: 3000\ntargets: []\n",
            name
        );
        std::fs::write(ws.join("this.yaml"), yaml).unwrap();

        // Entity model.rs
        std::fs::write(
            ws.join("api/src/entities/product/model.rs"),
            r#"
use this::prelude::*;

impl_data_entity!(
    Product,
    "product",
    ["name"],
    {
        name: String,
        price: f64,
    }
);
"#,
        )
        .unwrap();

        // Entity descriptor.rs
        std::fs::write(
            ws.join("api/src/entities/product/descriptor.rs"),
            r#"
impl EntityDescriptor for ProductDescriptor {
    fn entity_type(&self) -> &str { "product" }
    fn plural(&self) -> &str { "products" }
    fn build_routes(&self) -> Router {
        Router::new()
            .route("/products", get(list_products).post(create_product))
            .route("/products/{id}", get(get_product).put(update_product).delete(delete_product))
            .with_state(state)
    }
}
"#,
        )
        .unwrap();

        // Empty links.yaml
        std::fs::write(
            ws.join("api/config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        ws
    }

    // ── Generate client with scaffold workspace ──────────────────────

    #[test]
    fn test_generate_client_with_entities() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_generate_workspace(&tmp, "gen_test");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let output_path = ws.join("output/api-client.ts");
        let args = GenerateClientArgs {
            lang: "typescript".to_string(),
            output: Some(output_path.clone()),
        };

        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        // Check the output file was created
        assert!(output_path.exists(), "api-client.ts should be created");

        // Check it contains expected TypeScript content
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(
            content.contains("Product"),
            "Should contain Product interface"
        );
        assert!(
            content.contains("product"),
            "Should contain product references"
        );
    }

    // ── Auto-detect output with webapp target ────────────────────────

    #[test]
    fn test_generate_client_auto_detect_with_webapp() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_generate_workspace(&tmp, "gen_webapp");

        // Update this.yaml to include a webapp target
        let yaml = "name: gen_webapp\napi:\n  path: api\n  port: 3000\ntargets:\n  - target_type: webapp\n    framework: react\n    path: front\n";
        std::fs::write(ws.join("this.yaml"), yaml).unwrap();
        std::fs::create_dir_all(ws.join("front/src")).unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = GenerateClientArgs {
            lang: "typescript".to_string(),
            output: None, // auto-detect
        };

        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        // Should be written to front/src/api-client.ts
        let expected_path = ws.join("front/src/api-client.ts");
        assert!(
            expected_path.exists(),
            "Should auto-detect to front/src/api-client.ts"
        );
    }

    // ── Auto-detect output without webapp target ─────────────────────

    #[test]
    fn test_generate_client_auto_detect_no_webapp() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_generate_workspace(&tmp, "gen_no_webapp");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = GenerateClientArgs {
            lang: "typescript".to_string(),
            output: None, // auto-detect, no webapp target
        };

        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        // Should fall back to workspace root
        let expected_path = ws.join("api-client.ts");
        assert!(
            expected_path.exists(),
            "Should fall back to <workspace>/api-client.ts"
        );
    }

    // ── Error: not in a workspace ────────────────────────────────────

    #[test]
    fn test_generate_client_not_in_workspace_error() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = GenerateClientArgs {
            lang: "typescript".to_string(),
            output: None,
        };

        // Pass a directory with no this.yaml
        let result = run_in(args, &writer, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Not inside a this-rs workspace"),
            "Error should mention workspace: {}",
            err
        );
    }

    // ── Error: unsupported language ──────────────────────────────────

    #[test]
    fn test_generate_client_unsupported_language() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = GenerateClientArgs {
            lang: "python".to_string(),
            output: None,
        };

        let result = run_in(args, &writer, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unsupported language"),
            "Error should mention unsupported language: {}",
            err
        );
    }

    // ── Error: no entities found ─────────────────────────────────────

    #[test]
    fn test_generate_client_no_entities_error() {
        let tmp = TempDir::new().unwrap();
        let ws = tmp.path().join("empty_ws");
        std::fs::create_dir_all(ws.join("api/src")).unwrap();

        let yaml = "name: empty_ws\napi:\n  path: api\n  port: 3000\ntargets: []\n";
        std::fs::write(ws.join("this.yaml"), yaml).unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = GenerateClientArgs {
            lang: "typescript".to_string(),
            output: Some(ws.join("output.ts")),
        };

        let result = run_in(args, &writer, &ws);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("No entities found"),
            "Error should mention no entities: {}",
            err
        );
    }

    // ── Generate with multiple entities ──────────────────────────────

    #[test]
    fn test_generate_client_multiple_entities() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_generate_workspace(&tmp, "gen_multi");

        // Add a second entity: order
        let order_dir = ws.join("api/src/entities/order");
        std::fs::create_dir_all(&order_dir).unwrap();
        std::fs::write(
            order_dir.join("model.rs"),
            r#"
use this::prelude::*;

impl_data_entity!(
    Order,
    "order",
    ["reference"],
    {
        reference: String,
        total: f64,
    }
);
"#,
        )
        .unwrap();

        std::fs::write(
            order_dir.join("descriptor.rs"),
            r#"
impl EntityDescriptor for OrderDescriptor {
    fn entity_type(&self) -> &str { "order" }
    fn plural(&self) -> &str { "orders" }
    fn build_routes(&self) -> Router {
        Router::new()
            .route("/orders", get(list_orders).post(create_order))
            .route("/orders/{id}", get(get_order).put(update_order).delete(delete_order))
            .with_state(state)
    }
}
"#,
        )
        .unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let output_path = ws.join("api-client.ts");
        let args = GenerateClientArgs {
            lang: "typescript".to_string(),
            output: Some(output_path.clone()),
        };

        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("Product"), "Should contain Product");
        assert!(content.contains("Order"), "Should contain Order");
    }

    // ── Dry-run does not write ───────────────────────────────────────

    #[test]
    fn test_generate_client_dry_run() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_generate_workspace(&tmp, "gen_dry");
        let writer = crate::utils::file_writer::DryRunWriter::new();

        let output_path = ws.join("output/api-client.ts");
        let args = GenerateClientArgs {
            lang: "typescript".to_string(),
            output: Some(output_path.clone()),
        };

        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Dry run should succeed: {:?}", result.err());

        // The file should NOT exist in dry-run mode
        assert!(!output_path.exists(), "Dry run should not create the file");
    }
}
