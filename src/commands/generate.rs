//! `this generate client` — generate a typed API client from project introspection

use anyhow::{Result, bail};
use std::path::PathBuf;

use super::GenerateClientArgs;
use crate::codegen::introspect;
use crate::config::{TargetType, load_workspace_config};
use crate::utils::file_writer::FileWriter;
use crate::utils::{output, project};

pub fn run(args: GenerateClientArgs, writer: &dyn FileWriter) -> Result<()> {
    if args.lang != "typescript" {
        bail!(
            "Unsupported language: '{}'. Currently only 'typescript' is supported.",
            args.lang
        );
    }

    // Find workspace root and API directory
    let workspace_root = project::find_workspace_root().ok_or_else(|| {
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
}
