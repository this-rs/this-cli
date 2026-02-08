use anyhow::{Context, Result, bail};

use super::AddLinkArgs;
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

fn default_entity_auth() -> EntityAuth {
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

pub fn run(args: AddLinkArgs) -> Result<()> {
    let project_root = project::detect_project_root()?;
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
    std::fs::write(&links_path, &new_yaml)
        .with_context(|| format!("Failed to write: {}", links_path.display()))?;

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
