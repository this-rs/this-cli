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

    // ── helpers ──────────────────────────────────────────────────────────

    /// Write a proper initial links.yaml that can be deserialized into LinksConfig.
    fn write_valid_links_yaml(project_root: &std::path::Path) {
        let content = "entities: []\nlinks: []\nvalidation_rules: {}\n";
        std::fs::write(project_root.join("config/links.yaml"), content).unwrap();
    }

    /// Set up a workspace with two entities and a valid links.yaml.
    fn setup_project_with_entities(tmp: &TempDir, name: &str) -> std::path::PathBuf {
        let ws = crate::test_helpers::setup_test_workspace(tmp, name);
        let api = ws.join("api");
        crate::test_helpers::add_entity_to_project(&api.join("src"), "order");
        crate::test_helpers::add_entity_to_project(&api.join("src"), "invoice");
        write_valid_links_yaml(&api);
        ws
    }

    fn default_args(source: &str, target: &str) -> AddLinkArgs {
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

    fn writer() -> crate::mcp::handlers::McpFileWriter {
        crate::mcp::handlers::McpFileWriter::new()
    }

    // ── serde round-trip tests ──────────────────────────────────────────

    #[test]
    fn test_links_config_serde_roundtrip() {
        let config = LinksConfig {
            entities: vec![EntityConfig {
                singular: "order".into(),
                plural: "orders".into(),
                auth: default_entity_auth(),
            }],
            links: vec![LinkDefinition {
                link_type: "has_invoice".into(),
                source_type: "order".into(),
                target_type: "invoice".into(),
                forward_route_name: "invoices".into(),
                reverse_route_name: "order".into(),
                description: Some("Order -> Invoice relationship".into()),
                auth: None,
            }],
            validation_rules: std::collections::BTreeMap::new(),
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: LinksConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.entities.len(), 1);
        assert_eq!(parsed.entities[0].singular, "order");
        assert_eq!(parsed.entities[0].plural, "orders");
        assert_eq!(parsed.links.len(), 1);
        assert_eq!(parsed.links[0].link_type, "has_invoice");
    }

    #[test]
    fn test_entity_config_serde() {
        let entity = EntityConfig {
            singular: "product".into(),
            plural: "products".into(),
            auth: default_entity_auth(),
        };

        let yaml = serde_yaml::to_string(&entity).unwrap();
        let parsed: EntityConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.singular, "product");
        assert_eq!(parsed.plural, "products");
        assert_eq!(parsed.auth.list, "authenticated");
        assert_eq!(parsed.auth.get, "authenticated");
        assert_eq!(parsed.auth.create, "authenticated");
        assert_eq!(parsed.auth.update, "authenticated");
        assert_eq!(parsed.auth.delete, "authenticated");
    }

    #[test]
    fn test_link_definition_serde() {
        let link = LinkDefinition {
            link_type: "has_tag".into(),
            source_type: "article".into(),
            target_type: "tag".into(),
            forward_route_name: "tags".into(),
            reverse_route_name: "article".into(),
            description: Some("Article -> Tag relationship".into()),
            auth: Some(LinkAuth {
                list: "public".into(),
                get: "public".into(),
                create: "authenticated".into(),
                update: "authenticated".into(),
                delete: "admin".into(),
            }),
        };

        let yaml = serde_yaml::to_string(&link).unwrap();
        let parsed: LinkDefinition = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.link_type, "has_tag");
        assert_eq!(parsed.source_type, "article");
        assert_eq!(parsed.target_type, "tag");
        assert_eq!(parsed.forward_route_name, "tags");
        assert_eq!(parsed.reverse_route_name, "article");
        assert_eq!(
            parsed.description.as_deref(),
            Some("Article -> Tag relationship")
        );
        let auth = parsed.auth.unwrap();
        assert_eq!(auth.list, "public");
        assert_eq!(auth.delete, "admin");
    }

    // ── integration tests ───────────────────────────────────────────────

    #[test]
    fn test_add_link_creates_links_yaml() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_project_with_entities(&tmp, "link_basic");
        let w = writer();

        let result = run_in(default_args("order", "invoice"), &w, &ws);
        assert!(result.is_ok(), "run_in failed: {:?}", result.err());

        let yaml_content =
            std::fs::read_to_string(ws.join("api/config/links.yaml")).unwrap();
        let config: LinksConfig = serde_yaml::from_str(&yaml_content).unwrap();

        assert_eq!(config.links.len(), 1);
        assert_eq!(config.links[0].link_type, "has_invoice");
        assert_eq!(config.links[0].source_type, "order");
        assert_eq!(config.links[0].target_type, "invoice");
        assert_eq!(config.links[0].forward_route_name, "invoices");
        assert_eq!(config.links[0].reverse_route_name, "order");
        assert!(config.links[0].description.is_some());
    }

    #[test]
    fn test_add_link_with_custom_link_type() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_project_with_entities(&tmp, "link_custom_type");
        let w = writer();

        let args = AddLinkArgs {
            link_type: Some("belongs_to".into()),
            ..default_args("order", "invoice")
        };

        let result = run_in(args, &w, &ws);
        assert!(result.is_ok(), "run_in failed: {:?}", result.err());

        let yaml_content =
            std::fs::read_to_string(ws.join("api/config/links.yaml")).unwrap();
        let config: LinksConfig = serde_yaml::from_str(&yaml_content).unwrap();

        assert_eq!(config.links[0].link_type, "belongs_to");
    }

    #[test]
    fn test_add_link_with_custom_routes() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_project_with_entities(&tmp, "link_custom_routes");
        let w = writer();

        let args = AddLinkArgs {
            forward: Some("my_invoices".into()),
            reverse: Some("parent_order".into()),
            ..default_args("order", "invoice")
        };

        let result = run_in(args, &w, &ws);
        assert!(result.is_ok(), "run_in failed: {:?}", result.err());

        let yaml_content =
            std::fs::read_to_string(ws.join("api/config/links.yaml")).unwrap();
        let config: LinksConfig = serde_yaml::from_str(&yaml_content).unwrap();

        assert_eq!(config.links[0].forward_route_name, "my_invoices");
        assert_eq!(config.links[0].reverse_route_name, "parent_order");
    }

    #[test]
    fn test_add_link_no_validation_rule() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_project_with_entities(&tmp, "link_no_val");
        let w = writer();

        let args = AddLinkArgs {
            no_validation_rule: true,
            ..default_args("order", "invoice")
        };

        let result = run_in(args, &w, &ws);
        assert!(result.is_ok(), "run_in failed: {:?}", result.err());

        let yaml_content =
            std::fs::read_to_string(ws.join("api/config/links.yaml")).unwrap();
        let config: LinksConfig = serde_yaml::from_str(&yaml_content).unwrap();

        assert!(
            config.validation_rules.is_empty(),
            "Expected no validation rules, got: {:?}",
            config.validation_rules
        );
    }

    #[test]
    fn test_add_link_adds_entity_configs() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_project_with_entities(&tmp, "link_entities");
        let w = writer();

        let result = run_in(default_args("order", "invoice"), &w, &ws);
        assert!(result.is_ok(), "run_in failed: {:?}", result.err());

        let yaml_content =
            std::fs::read_to_string(ws.join("api/config/links.yaml")).unwrap();
        let config: LinksConfig = serde_yaml::from_str(&yaml_content).unwrap();

        // Both entities should be added since initial links.yaml has none
        assert_eq!(config.entities.len(), 2);
        assert!(config.entities.iter().any(|e| e.singular == "order"));
        assert!(config.entities.iter().any(|e| e.singular == "invoice"));

        // Verify pluralization
        let order_entity = config
            .entities
            .iter()
            .find(|e| e.singular == "order")
            .unwrap();
        assert_eq!(order_entity.plural, "orders");
        let invoice_entity = config
            .entities
            .iter()
            .find(|e| e.singular == "invoice")
            .unwrap();
        assert_eq!(invoice_entity.plural, "invoices");
    }

    #[test]
    fn test_add_link_duplicate_error() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_project_with_entities(&tmp, "link_dup");
        let w = writer();

        // First add should succeed
        let result = run_in(default_args("order", "invoice"), &w, &ws);
        assert!(result.is_ok(), "First add failed: {:?}", result.err());

        // Second add of the same link should fail
        let result2 = run_in(default_args("order", "invoice"), &w, &ws);
        assert!(result2.is_err());
        let err_msg = result2.unwrap_err().to_string();
        assert!(
            err_msg.contains("already exists"),
            "Expected 'already exists' in error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_add_link_outside_project_error() {
        let tmp = TempDir::new().unwrap();
        let w = writer();

        // tmp.path() has no project scaffold at all
        let result = run_in(default_args("order", "invoice"), &w, tmp.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Not inside a this-rs project"),
            "Expected project detection error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_add_link_adds_validation_rule_by_default() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_project_with_entities(&tmp, "link_val_rule");
        let w = writer();

        let result = run_in(default_args("order", "invoice"), &w, &ws);
        assert!(result.is_ok(), "run_in failed: {:?}", result.err());

        let yaml_content =
            std::fs::read_to_string(ws.join("api/config/links.yaml")).unwrap();
        let config: LinksConfig = serde_yaml::from_str(&yaml_content).unwrap();

        assert!(
            config.validation_rules.contains_key("has_invoice"),
            "Expected validation rule for 'has_invoice', got: {:?}",
            config.validation_rules
        );
        let rules = &config.validation_rules["has_invoice"];
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].source, "order");
        assert_eq!(rules[0].targets, vec!["invoice".to_string()]);
    }
}
