use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::AddLinkArgs;
use crate::utils::file_writer::FileWriter;
use crate::utils::{naming, output, project};

/// Represents the links.yaml config structure (subset for CLI manipulation)
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LinksConfig {
    #[serde(default)]
    pub entities: Vec<EntityConfig>,
    #[serde(default)]
    pub links: Vec<LinkDefinition>,
    #[serde(default)]
    pub validation_rules: std::collections::BTreeMap<String, Vec<ValidationRule>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EntityConfig {
    pub singular: String,
    pub plural: String,
    #[serde(default = "default_entity_auth")]
    pub auth: EntityAuth,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EntityAuth {
    #[serde(default = "default_auth")]
    pub list: String,
    #[serde(default = "default_auth")]
    pub get: String,
    #[serde(default = "default_auth")]
    pub create: String,
    #[serde(default = "default_auth")]
    pub update: String,
    #[serde(default = "default_auth")]
    pub delete: String,
}

fn default_auth() -> String {
    "authenticated".to_string()
}

pub fn default_entity_auth() -> EntityAuth {
    EntityAuth {
        list: default_auth(),
        get: default_auth(),
        create: default_auth(),
        update: default_auth(),
        delete: default_auth(),
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LinkDefinition {
    pub link_type: String,
    pub source_type: String,
    pub target_type: String,
    pub forward_route_name: String,
    pub reverse_route_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<LinkAuth>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LinkAuth {
    #[serde(default = "default_auth")]
    pub list: String,
    #[serde(default = "default_auth")]
    pub get: String,
    #[serde(default = "default_auth")]
    pub create: String,
    #[serde(default = "default_auth")]
    pub update: String,
    #[serde(default = "default_auth")]
    pub delete: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ValidationRule {
    pub source: String,
    pub targets: Vec<String>,
}

pub fn run(args: AddLinkArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
    run_in(args, writer, &cwd)
}

/// Run the add link command with an explicit starting directory.
/// This avoids relying on the process-global CWD, making it safe for parallel tests.
pub(crate) fn run_in(
    args: AddLinkArgs,
    writer: &dyn FileWriter,
    cwd: &std::path::Path,
) -> Result<()> {
    let project_root = project::detect_project_root_from(cwd)?;
    let links_path = project_root.join("config/links.yaml");

    if !links_path.exists() {
        bail!(
            "config/links.yaml not found at {}. Run 'this init' first or create it manually.",
            links_path.display()
        );
    }

    let source = naming::to_snake_case(&args.source);
    let target = naming::to_snake_case(&args.target);
    let link_type = args.link_type.unwrap_or_else(|| format!("has_{}", &target));
    let forward = args.forward.unwrap_or_else(|| naming::pluralize(&target));
    let reverse = args.reverse.unwrap_or_else(|| source.clone());

    if writer.is_dry_run() {
        println!("🔍 {}", "Dry run — no files will be written".cyan().bold());
        println!();
    }

    output::print_step(&format!(
        "Adding link '{} -> {}' to config/links.yaml...",
        &source, &target
    ));

    // Read and parse existing config
    let yaml_content = std::fs::read_to_string(&links_path)
        .with_context(|| format!("Failed to read: {}", links_path.display()))?;
    let mut config: LinksConfig =
        serde_yaml::from_str(&yaml_content).with_context(|| "Failed to parse links.yaml")?;

    // Check for duplicate link
    let duplicate = config
        .links
        .iter()
        .any(|l| l.link_type == link_type && l.source_type == source && l.target_type == target);
    if duplicate {
        bail!(
            "Link '{}' from '{}' to '{}' already exists in links.yaml",
            link_type,
            source,
            target
        );
    }

    // Ensure entities exist
    let source_plural = naming::pluralize(&source);
    let target_plural = naming::pluralize(&target);

    if !config.entities.iter().any(|e| e.singular == source) {
        config.entities.push(EntityConfig {
            singular: source.clone(),
            plural: source_plural.clone(),
            auth: default_entity_auth(),
        });
        output::print_info(&format!("Added entity config for: {}", &source));
    }
    if !config.entities.iter().any(|e| e.singular == target) {
        config.entities.push(EntityConfig {
            singular: target.clone(),
            plural: target_plural.clone(),
            auth: default_entity_auth(),
        });
        output::print_info(&format!("Added entity config for: {}", &target));
    }

    // Add link definition
    let description = args.description.or_else(|| {
        Some(format!(
            "{} -> {} relationship",
            naming::to_pascal_case(&source),
            naming::to_pascal_case(&target)
        ))
    });

    config.links.push(LinkDefinition {
        link_type: link_type.clone(),
        source_type: source.clone(),
        target_type: target.clone(),
        forward_route_name: forward.clone(),
        reverse_route_name: reverse.clone(),
        description,
        auth: Some(LinkAuth {
            list: default_auth(),
            get: default_auth(),
            create: default_auth(),
            update: default_auth(),
            delete: default_auth(),
        }),
    });

    // Add validation rule
    if !args.no_validation_rule {
        config
            .validation_rules
            .entry(link_type.clone())
            .or_default()
            .push(ValidationRule {
                source: source.clone(),
                targets: vec![target.clone()],
            });
    }

    // Write back
    let new_yaml =
        serde_yaml::to_string(&config).with_context(|| "Failed to serialize links.yaml")?;
    writer.update_file(&links_path, &yaml_content, &new_yaml)?;

    output::print_info(&format!("Link type: {}", &link_type));
    output::print_info(&format!(
        "Forward route: {} (on /{}/{{id}}/{})",
        &forward, &source_plural, &forward
    ));
    output::print_info(&format!(
        "Reverse route: {} (on /{}/{{id}}/{})",
        &reverse, &target_plural, &reverse
    ));

    output::print_success("Link added to config/links.yaml!");

    output::print_next_steps(&[
        "Routes that will be generated:",
        &format!(
            "  GET    /{}/{{id}}/{}        - List {} for a {}",
            &source_plural, &forward, &target_plural, &source
        ),
        &format!(
            "  POST   /{}/{{id}}/{}        - Link a {} to a {}",
            &source_plural, &forward, &target, &source
        ),
        &format!(
            "  GET    /{}/{{id}}/{}         - Get {} for a {}",
            &target_plural, &reverse, &source, &target
        ),
    ]);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AddLinkArgs;
    use tempfile::TempDir;

    /// Set up a minimal this-rs project structure that `detect_project_root_from` can find.
    /// The function looks for a Cargo.toml containing `[dependencies]` and `this`,
    /// then add_link needs `config/links.yaml` to exist.
    fn setup_link_project(tmp: &TempDir) -> std::path::PathBuf {
        let project = tmp.path().join("linktest");
        std::fs::create_dir_all(project.join("src")).unwrap();

        // Cargo.toml that detect_project_root_from will match
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"linktest\"\n\n[dependencies]\nthis = \"0.1\"\n",
        )
        .unwrap();

        // config/links.yaml (empty initial state)
        let config_dir = project.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        project
    }

    fn make_args(source: &str, target: &str) -> AddLinkArgs {
        AddLinkArgs {
            source: source.to_string(),
            target: target.to_string(),
            link_type: None,
            forward: None,
            reverse: None,
            description: None,
            no_validation_rule: false,
        }
    }

    fn read_links_config(project: &std::path::Path) -> LinksConfig {
        let content = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
        serde_yaml::from_str(&content).unwrap()
    }

    // ── Basic link creation ──────────────────────────────────────────

    #[test]
    fn test_add_basic_link() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("order", "invoice");
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_links_config(&project);
        assert_eq!(config.links.len(), 1);
        assert_eq!(config.links[0].link_type, "has_invoice");
        assert_eq!(config.links[0].source_type, "order");
        assert_eq!(config.links[0].target_type, "invoice");
        assert_eq!(config.links[0].forward_route_name, "invoices");
        assert_eq!(config.links[0].reverse_route_name, "order");
    }

    // ── Custom link_type, forward, reverse ───────────────────────────

    #[test]
    fn test_add_link_with_custom_names() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddLinkArgs {
            source: "user".to_string(),
            target: "role".to_string(),
            link_type: Some("assigned_role".to_string()),
            forward: Some("assigned_roles".to_string()),
            reverse: Some("assigned_users".to_string()),
            description: None,
            no_validation_rule: false,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_links_config(&project);
        assert_eq!(config.links[0].link_type, "assigned_role");
        assert_eq!(config.links[0].forward_route_name, "assigned_roles");
        assert_eq!(config.links[0].reverse_route_name, "assigned_users");
    }

    // ── Custom description ───────────────────────────────────────────

    #[test]
    fn test_add_link_with_description() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddLinkArgs {
            source: "product".to_string(),
            target: "category".to_string(),
            link_type: None,
            forward: None,
            reverse: None,
            description: Some("Products belong to categories".to_string()),
            no_validation_rule: false,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_links_config(&project);
        assert_eq!(
            config.links[0].description.as_deref(),
            Some("Products belong to categories")
        );
    }

    // ── Duplicate link detection ─────────────────────────────────────

    #[test]
    fn test_add_duplicate_link_errors() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // First link succeeds
        let args = make_args("order", "invoice");
        run_in(args, &writer, &project).unwrap();

        // Same link again should fail
        let args2 = make_args("order", "invoice");
        let result = run_in(args2, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention duplicate: {}",
            err
        );
    }

    // ── Default values ───────────────────────────────────────────────

    #[test]
    fn test_default_link_type_is_has_target() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("order", "product");
        run_in(args, &writer, &project).unwrap();

        let config = read_links_config(&project);
        assert_eq!(config.links[0].link_type, "has_product");
    }

    #[test]
    fn test_default_forward_is_pluralized_target() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("order", "category");
        run_in(args, &writer, &project).unwrap();

        let config = read_links_config(&project);
        // "category" pluralizes to "categories"
        assert_eq!(config.links[0].forward_route_name, "categories");
    }

    #[test]
    fn test_default_reverse_is_source() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("order", "invoice");
        run_in(args, &writer, &project).unwrap();

        let config = read_links_config(&project);
        assert_eq!(config.links[0].reverse_route_name, "order");
    }

    // ── Entities auto-created ────────────────────────────────────────

    #[test]
    fn test_entities_auto_created() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("order", "invoice");
        run_in(args, &writer, &project).unwrap();

        let config = read_links_config(&project);
        assert_eq!(config.entities.len(), 2);

        let order = config.entities.iter().find(|e| e.singular == "order");
        assert!(order.is_some(), "Should have order entity");
        assert_eq!(order.unwrap().plural, "orders");

        let invoice = config.entities.iter().find(|e| e.singular == "invoice");
        assert!(invoice.is_some(), "Should have invoice entity");
        assert_eq!(invoice.unwrap().plural, "invoices");
    }

    // ── LinksConfig serde round-trip ─────────────────────────────────

    #[test]
    fn test_links_config_serde_roundtrip() {
        let config = LinksConfig {
            entities: vec![EntityConfig {
                singular: "order".to_string(),
                plural: "orders".to_string(),
                auth: default_entity_auth(),
            }],
            links: vec![LinkDefinition {
                link_type: "has_invoice".to_string(),
                source_type: "order".to_string(),
                target_type: "invoice".to_string(),
                forward_route_name: "invoices".to_string(),
                reverse_route_name: "order".to_string(),
                description: Some("Order -> Invoice".to_string()),
                auth: Some(LinkAuth {
                    list: "authenticated".to_string(),
                    get: "authenticated".to_string(),
                    create: "authenticated".to_string(),
                    update: "authenticated".to_string(),
                    delete: "authenticated".to_string(),
                }),
            }],
            validation_rules: {
                let mut m = std::collections::BTreeMap::new();
                m.insert(
                    "has_invoice".to_string(),
                    vec![ValidationRule {
                        source: "order".to_string(),
                        targets: vec!["invoice".to_string()],
                    }],
                );
                m
            },
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: LinksConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.entities.len(), 1);
        assert_eq!(parsed.entities[0].singular, "order");
        assert_eq!(parsed.links.len(), 1);
        assert_eq!(parsed.links[0].link_type, "has_invoice");
        assert_eq!(parsed.validation_rules.len(), 1);
        assert!(parsed.validation_rules.contains_key("has_invoice"));
    }

    // ── --no-validation-rule flag ────────────────────────────────────

    #[test]
    fn test_no_validation_rule_flag() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddLinkArgs {
            source: "order".to_string(),
            target: "invoice".to_string(),
            link_type: None,
            forward: None,
            reverse: None,
            description: None,
            no_validation_rule: true,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_links_config(&project);
        // Link should exist but no validation rule
        assert_eq!(config.links.len(), 1);
        assert!(
            config.validation_rules.is_empty(),
            "Should have no validation rules when --no-validation-rule is set"
        );
    }

    // ── With validation rule (default) ───────────────────────────────

    #[test]
    fn test_validation_rule_added_by_default() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("order", "invoice");
        run_in(args, &writer, &project).unwrap();

        let config = read_links_config(&project);
        assert!(!config.validation_rules.is_empty());
        let rules = config.validation_rules.get("has_invoice");
        assert!(
            rules.is_some(),
            "Should have validation rules for has_invoice"
        );
        let rules = rules.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].source, "order");
        assert_eq!(rules[0].targets, vec!["invoice"]);
    }

    // ── Default description auto-generated ───────────────────────────

    #[test]
    fn test_default_description_auto_generated() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("order", "invoice");
        run_in(args, &writer, &project).unwrap();

        let config = read_links_config(&project);
        assert_eq!(
            config.links[0].description.as_deref(),
            Some("Order -> Invoice relationship")
        );
    }

    // ── Missing links.yaml errors ────────────────────────────────────

    #[test]
    fn test_missing_links_yaml_errors() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("nolinktest");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"nolinktest\"\n\n[dependencies]\nthis = \"0.1\"\n",
        )
        .unwrap();
        // No config/links.yaml

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = make_args("order", "invoice");
        let result = run_in(args, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("config/links.yaml not found"),
            "Error should mention missing links.yaml: {}",
            err
        );
    }

    // ── Snake-case normalization ─────────────────────────────────────

    #[test]
    fn test_source_target_normalized_to_snake_case() {
        let tmp = TempDir::new().unwrap();
        let project = setup_link_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // Pass PascalCase names — should be normalized
        let args = AddLinkArgs {
            source: "OrderItem".to_string(),
            target: "ProductCategory".to_string(),
            link_type: None,
            forward: None,
            reverse: None,
            description: None,
            no_validation_rule: false,
        };
        run_in(args, &writer, &project).unwrap();

        let config = read_links_config(&project);
        assert_eq!(config.links[0].source_type, "order_item");
        assert_eq!(config.links[0].target_type, "product_category");
        assert_eq!(config.links[0].link_type, "has_product_category");
    }
}
