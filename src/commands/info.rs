use std::path::Path;

use anyhow::{Context, Result};
use colored::Colorize;
use serde::Serialize;

use crate::config;
use crate::utils::{markers, project};

/// Parsed entity info from scanning the project
#[derive(Debug, Serialize)]
pub struct EntityInfo {
    pub name: String,
    pub fields: Vec<(String, String)>, // (name, type)
    pub is_validated: bool,
}

/// Parsed link info from links.yaml
#[derive(Debug, Serialize)]
pub struct LinkInfo {
    pub link_type: String,
    pub source: String,
    pub target: String,
    pub forward_route: String,
    pub reverse_route: String,
}

/// Coherence check result
#[derive(Debug, Serialize)]
pub struct CoherenceStatus {
    pub module_registered: usize,
    pub module_total: usize,
    pub stores_configured: usize,
    pub stores_total: usize,
    pub links_valid: bool,
    pub links_issues: Vec<String>,
}

/// Workspace information (only present if inside a workspace)
#[derive(Debug, Serialize)]
pub struct WorkspaceInfo {
    pub name: String,
    pub api_path: String,
    pub api_port: u16,
    pub targets: Vec<TargetInfo>,
}

/// Target information within a workspace
#[derive(Debug, Serialize)]
pub struct TargetInfo {
    pub target_type: String,
    pub path: String,
    pub framework: Option<String>,
    pub runtime: Option<String>,
}

/// Feature flags detected from this-rs dependency
#[derive(Debug, Serialize)]
pub struct FeatureFlags {
    pub websocket: bool,
    pub grpc: bool,
}

/// Complete project information — returned by collect_info() for structured (MCP) use
#[derive(Debug, Serialize)]
pub struct ProjectInfo {
    pub project_name: String,
    pub this_version: String,
    pub features: FeatureFlags,
    pub entities: Vec<EntityInfo>,
    pub links: Vec<LinkInfo>,
    pub coherence: CoherenceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceInfo>,
}

/// Collect project information as a structured object.
/// Used by the MCP handler for JSON serialization.
pub fn collect_info() -> Result<ProjectInfo> {
    let project_root = project::detect_project_root()?;
    let (project_name, this_version) = parse_cargo_toml(&project_root)?;
    let features = detect_this_features(&project_root);
    let entities = scan_entities(&project_root)?;
    let links = parse_links_yaml(&project_root)?;
    let coherence = check_coherence(&project_root, &entities)?;

    // Detect workspace context
    let workspace = detect_workspace_info();

    Ok(ProjectInfo {
        project_name,
        this_version,
        features,
        entities,
        links,
        coherence,
        workspace,
    })
}

/// Detect workspace context by looking for this.yaml
fn detect_workspace_info() -> Option<WorkspaceInfo> {
    let ws_root = project::find_workspace_root()?;
    let this_yaml_path = ws_root.join("this.yaml");
    let ws_config = config::load_workspace_config(&this_yaml_path).ok()?;

    let targets = ws_config
        .targets
        .iter()
        .map(|t| TargetInfo {
            target_type: t.target_type.to_string(),
            path: t.path.clone(),
            framework: t.framework.clone(),
            runtime: t.runtime.clone(),
        })
        .collect();

    Some(WorkspaceInfo {
        name: ws_config.name,
        api_path: ws_config.api.path,
        api_port: ws_config.api.port,
        targets,
    })
}

pub fn run() -> Result<()> {
    let info = collect_info()?;

    let project_name = &info.project_name;
    let this_version = &info.this_version;
    let entities = &info.entities;
    let links = &info.links;
    let coherence = &info.coherence;

    // Display
    println!();
    println!("{} Project: {}", "📦".bold(), project_name.cyan().bold());
    println!("   Framework: this-rs {}", this_version.dimmed());
    if info.features.websocket {
        println!("   WebSocket: {}", "✓ enabled".green());
    } else {
        println!("   WebSocket: {}", "✗ disabled".dimmed());
    }
    if info.features.grpc {
        println!("   gRPC:      {}", "✓ enabled".green());
    } else {
        println!("   gRPC:      {}", "✗ disabled".dimmed());
    }
    println!();

    // Workspace section (only if inside a workspace)
    if let Some(ws) = &info.workspace {
        println!("{} Workspace: {}", "🏗️".bold(), ws.name.cyan().bold());
        println!(
            "   API: {} (port {})",
            ws.api_path.dimmed(),
            ws.api_port.to_string().dimmed()
        );
        if ws.targets.is_empty() {
            println!("   Targets: {}", "none".dimmed());
        } else {
            println!("   Targets ({}):", ws.targets.len());
            for target in &ws.targets {
                let detail = match (&target.framework, &target.runtime) {
                    (Some(fw), _) => format!(" ({})", fw),
                    (_, Some(rt)) => format!(" ({})", rt),
                    _ => String::new(),
                };
                println!(
                    "     {} {} → {}{}",
                    "•".dimmed(),
                    target.target_type.bold(),
                    target.path.dimmed(),
                    detail.dimmed()
                );
            }
        }
        println!();
    }

    // Entities section
    if entities.is_empty() {
        println!("{} Entities: {}", "📋".bold(), "none".dimmed());
    } else {
        println!("{} Entities ({}):", "📋".bold(), entities.len());
        for entity in entities {
            let fields_str = if entity.fields.is_empty() {
                "no fields".dimmed().to_string()
            } else {
                entity
                    .fields
                    .iter()
                    .map(|(name, _): &(String, String)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let validated_tag = if entity.is_validated {
                " [validated]".yellow().to_string()
            } else {
                String::new()
            };
            println!(
                "   {} {} (fields: {}){}",
                "•".dimmed(),
                entity.name.bold(),
                fields_str,
                validated_tag
            );
        }
    }
    println!();

    // Links section
    if links.is_empty() {
        println!("{} Links: {}", "🔗".bold(), "none".dimmed());
    } else {
        println!("{} Links ({}):", "🔗".bold(), links.len());
        for link in links {
            let source_plural = crate::utils::naming::pluralize(&link.source);
            let target_plural = crate::utils::naming::pluralize(&link.target);
            println!(
                "   {} {} {} {} ({})",
                "•".dimmed(),
                link.source.bold(),
                "→".dimmed(),
                link.target.bold(),
                link.link_type.dimmed()
            );
            println!(
                "     {} Forward: /{}/{{id}}/{}",
                "↳".dimmed(),
                source_plural,
                link.forward_route
            );
            println!(
                "     {} Reverse: /{}/{{id}}/{}",
                "↳".dimmed(),
                target_plural,
                link.reverse_route
            );
        }
    }
    println!();

    // Status section
    println!("{} Status:", "📊".bold());

    // Module registration
    let check =
        if coherence.module_total > 0 && coherence.module_registered < coherence.module_total {
            "⚠️"
        } else {
            "✅"
        };
    if coherence.module_total == 0 {
        println!("   {check} Module: {}", "No entities to register".dimmed());
    } else {
        println!(
            "   {check} Module: {}/{} entities registered",
            coherence.module_registered, coherence.module_total
        );
    }

    // Stores
    let check =
        if coherence.stores_total > 0 && coherence.stores_configured < coherence.stores_total {
            "⚠️"
        } else {
            "✅"
        };
    if coherence.stores_total == 0 {
        println!("   {check} Stores: {}", "No stores to configure".dimmed());
    } else {
        println!(
            "   {check} Stores: {}/{} stores configured",
            coherence.stores_configured, coherence.stores_total
        );
    }

    // Links validity
    if coherence.links_valid {
        println!("   ✅ Links: Valid configuration");
    } else {
        println!("   ⚠️ Links: {}", "Issues found".yellow());
        for issue in &coherence.links_issues {
            println!("     {} {}", "→".dimmed(), issue);
        }
    }

    println!();

    Ok(())
}

/// Parse Cargo.toml to extract project name and this-rs version
fn parse_cargo_toml(project_root: &Path) -> Result<(String, String)> {
    let cargo_path = project_root.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_path)
        .with_context(|| format!("Failed to read {}", cargo_path.display()))?;

    let doc = content
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| "Failed to parse Cargo.toml")?;

    let project_name = doc
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Try to find this-rs version from dependencies
    let this_version = extract_this_version(&doc).unwrap_or_else(|| "unknown".to_string());

    Ok((project_name, this_version))
}

/// Extract this-rs version from Cargo.toml dependencies
fn extract_this_version(doc: &toml_edit::DocumentMut) -> Option<String> {
    let deps = doc.get("dependencies")?;

    // Try `this` key (could be a table or inline table)
    let this_dep = deps.get("this")?;

    // Simple string version: this = "0.0.6"
    if let Some(version) = this_dep.as_str() {
        return Some(format!("v{}", version));
    }

    // Table form: this = { package = "this-rs", version = "0.0.6" }
    if let Some(version) = this_dep.get("version").and_then(|v| v.as_str()) {
        return Some(format!("v{}", version));
    }

    // Path dependency: this = { package = "this-rs", path = "../this" }
    if let Some(path) = this_dep.get("path").and_then(|v| v.as_str()) {
        return Some(format!("(path: {})", path));
    }

    None
}

/// Detect which this-rs features are enabled in Cargo.toml
pub fn detect_this_features(project_root: &Path) -> FeatureFlags {
    let cargo_path = project_root.join("Cargo.toml");
    let content = match std::fs::read_to_string(&cargo_path) {
        Ok(c) => c,
        Err(_) => {
            return FeatureFlags {
                websocket: false,
                grpc: false,
            };
        }
    };

    let doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => {
            return FeatureFlags {
                websocket: false,
                grpc: false,
            };
        }
    };

    let features_array = doc
        .get("dependencies")
        .and_then(|deps| deps.get("this"))
        .and_then(|this_dep| this_dep.get("features"))
        .and_then(|features| features.as_array());

    let websocket =
        features_array.is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some("websocket")));

    let grpc = features_array.is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some("grpc")));

    FeatureFlags { websocket, grpc }
}

/// Scan src/entities/ to discover entities and their fields
fn scan_entities(project_root: &Path) -> Result<Vec<EntityInfo>> {
    let entities_dir = project_root.join("src/entities");
    if !entities_dir.exists() {
        return Ok(vec![]);
    }

    let mut entities = Vec::new();

    let mut entries: Vec<_> = std::fs::read_dir(&entities_dir)
        .with_context(|| "Failed to read src/entities/")?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let entity_name = entry.file_name().to_string_lossy().to_string();
        let model_path = entry.path().join("model.rs");

        if !model_path.exists() {
            continue;
        }

        let model_content = std::fs::read_to_string(&model_path)
            .with_context(|| format!("Failed to read {}", model_path.display()))?;

        let fields = parse_model_fields(&model_content);
        let is_validated = model_content.contains("impl_data_entity_validated!");

        entities.push(EntityInfo {
            name: entity_name,
            fields,
            is_validated,
        });
    }

    Ok(entities)
}

/// Parse fields from a model.rs file (from impl_data_entity! macro invocation)
fn parse_model_fields(content: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();

    // Look for the fields block inside the macro: { field: Type, ... }
    // The pattern is: after the indexed fields array, there's a { ... } block
    let mut in_fields_block = false;
    let mut brace_depth = 0;
    let mut found_opening_brace = false;
    let mut brace_count = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        // Count braces to find the fields block (the second { ... } block or the one after [])
        if !in_fields_block {
            // Look for a line that has an opening brace after the macro header
            if trimmed == "{" || (trimmed.starts_with('{') && !trimmed.contains("impl_data_entity"))
            {
                brace_count += 1;
                // The first standalone { after the macro invocation args is the fields block
                // In impl_data_entity!(Name, "type", [...], { fields })
                // We want the { that starts the fields block
                if brace_count >= 1 && !found_opening_brace {
                    // Check if previous context suggests this is the fields block
                    in_fields_block = true;
                    found_opening_brace = true;
                    brace_depth = 1;
                    continue;
                }
            }
            continue;
        }

        // Inside fields block
        for ch in trimmed.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
        }

        if brace_depth == 0 {
            break;
        }

        // Parse field: "name: Type," or "name: Type"
        let field_line = trimmed.trim_end_matches(',').trim();
        if field_line.is_empty() || field_line.starts_with("//") {
            continue;
        }

        if let Some((name, typ)) = field_line.split_once(':') {
            let name = name.trim().to_string();
            let typ = typ.trim().to_string();
            if !name.is_empty() && !typ.is_empty() {
                fields.push((name, typ));
            }
        }
    }

    fields
}

/// Parse config/links.yaml for link definitions
fn parse_links_yaml(project_root: &Path) -> Result<Vec<LinkInfo>> {
    let links_path = project_root.join("config/links.yaml");
    if !links_path.exists() {
        return Ok(vec![]);
    }

    let content =
        std::fs::read_to_string(&links_path).with_context(|| "Failed to read config/links.yaml")?;

    let config: super::add_link::LinksConfig =
        serde_yaml::from_str(&content).with_context(|| "Failed to parse links.yaml")?;

    let links = config
        .links
        .into_iter()
        .map(|l| LinkInfo {
            link_type: l.link_type,
            source: l.source_type,
            target: l.target_type,
            forward_route: l.forward_route_name,
            reverse_route: l.reverse_route_name,
        })
        .collect();

    Ok(links)
}

/// Check coherence between entities, module.rs, and stores.rs
fn check_coherence(project_root: &Path, entities: &[EntityInfo]) -> Result<CoherenceStatus> {
    let entity_names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    let total = entity_names.len();

    // Check module.rs registration
    let module_registered = check_module_registration(project_root, &entity_names);

    // Check stores.rs configuration
    let stores_configured = check_stores_configuration(project_root, &entity_names);

    // Check links validity
    let (links_valid, links_issues) = check_links_validity(project_root, &entity_names);

    Ok(CoherenceStatus {
        module_registered,
        module_total: total,
        stores_configured,
        stores_total: total,
        links_valid,
        links_issues,
    })
}

/// Count how many entities are registered in module.rs
fn check_module_registration(project_root: &Path, entity_names: &[&str]) -> usize {
    let module_path = project_root.join("src/module.rs");
    if !module_path.exists() {
        return 0;
    }

    let content = match std::fs::read_to_string(&module_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    entity_names
        .iter()
        .filter(|name| {
            // Check if entity is in entity_types via marker or direct string
            let needle = format!("\"{}\"", name);
            if content.contains("[this:entity_types]") {
                markers::has_line_after_marker(&content, "[this:entity_types]", &needle)
            } else {
                // Fallback: check if the entity name string appears in entity_types() method
                content.contains(&needle)
            }
        })
        .count()
}

/// Count how many entities have stores configured in stores.rs
fn check_stores_configuration(project_root: &Path, entity_names: &[&str]) -> usize {
    let stores_path = project_root.join("src/stores.rs");
    if !stores_path.exists() {
        return 0;
    }

    let content = match std::fs::read_to_string(&stores_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };

    entity_names
        .iter()
        .filter(|name| {
            let plural = crate::utils::naming::pluralize(name);
            let needle = format!("{}_store:", plural);
            if content.contains("[this:store_fields]") {
                markers::has_line_after_marker(&content, "[this:store_fields]", &needle)
            } else {
                content.contains(&needle)
            }
        })
        .count()
}

/// Check if links.yaml references only existing entities
fn check_links_validity(project_root: &Path, entity_names: &[&str]) -> (bool, Vec<String>) {
    let links_path = project_root.join("config/links.yaml");
    if !links_path.exists() {
        return (true, vec![]);
    }

    let content = match std::fs::read_to_string(&links_path) {
        Ok(c) => c,
        Err(_) => return (true, vec![]),
    };

    let config: super::add_link::LinksConfig = match serde_yaml::from_str(&content) {
        Ok(c) => c,
        Err(e) => return (false, vec![format!("Invalid YAML: {}", e)]),
    };

    let mut issues = Vec::new();
    for link in &config.links {
        if !entity_names.contains(&link.source_type.as_str()) {
            issues.push(format!(
                "'{}' references unknown source entity '{}'",
                link.link_type, link.source_type
            ));
        }
        if !entity_names.contains(&link.target_type.as_str()) {
            issues.push(format!(
                "'{}' references unknown target entity '{}'",
                link.link_type, link.target_type
            ));
        }
    }

    (issues.is_empty(), issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_fields_basic() {
        let content = r#"use this::prelude::*;

impl_data_entity!(
    Product,
    "product",
    ["name"],
    {
        sku: String,
        price: f64,
        description: Option<String>,
    }
);
"#;
        let fields = parse_model_fields(content);
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0], ("sku".to_string(), "String".to_string()));
        assert_eq!(fields[1], ("price".to_string(), "f64".to_string()));
        assert_eq!(
            fields[2],
            ("description".to_string(), "Option<String>".to_string())
        );
    }

    #[test]
    fn test_parse_model_fields_empty() {
        let content = r#"use this::prelude::*;

impl_data_entity!(
    Product,
    "product",
    ["name"],
    {
    }
);
"#;
        let fields = parse_model_fields(content);
        assert_eq!(fields.len(), 0);
    }

    #[test]
    fn test_parse_model_fields_validated() {
        let content = r#"use this::prelude::*;

impl_data_entity_validated!(
    Product,
    "product",
    ["name"],
    {
        sku: String,
        price: f64,
    },
    validate: {
        create: {
            sku: [required],
        },
    }
);
"#;
        let fields = parse_model_fields(content);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], ("sku".to_string(), "String".to_string()));
        assert_eq!(fields[1], ("price".to_string(), "f64".to_string()));
    }
}
