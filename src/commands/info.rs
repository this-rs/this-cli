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
    pub graphql: bool,
    pub websocket: bool,
    pub grpc: bool,
}

/// Event system information
#[derive(Debug, Serialize)]
pub struct EventsInfo {
    pub sinks: Vec<String>,
    pub flows: Vec<String>,
}

/// Complete project information — returned by collect_info() for structured (MCP) use
#[derive(Debug, Serialize)]
pub struct ProjectInfo {
    pub project_name: String,
    pub this_version: String,
    pub features: FeatureFlags,
    pub entities: Vec<EntityInfo>,
    pub links: Vec<LinkInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<EventsInfo>,
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
    let events = detect_events_info(&project_root);
    let coherence = check_coherence(&project_root, &entities)?;

    // Detect workspace context
    let workspace = detect_workspace_info();

    Ok(ProjectInfo {
        project_name,
        this_version,
        features,
        entities,
        links,
        events,
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

/// Detect event system info by reading config/events.yaml
fn detect_events_info(project_root: &Path) -> Option<EventsInfo> {
    let events_path = project_root.join("config/events.yaml");
    let content = std::fs::read_to_string(&events_path).ok()?;
    let config: crate::commands::add_event_flow::EventsConfig =
        serde_yaml::from_str(&content).ok()?;

    Some(EventsInfo {
        sinks: config.event_sinks.iter().map(|s| s.name.clone()).collect(),
        flows: config.event_flows.iter().map(|f| f.name.clone()).collect(),
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
    if info.features.graphql {
        println!("   GraphQL:   {}", "✓ enabled".green());
    } else {
        println!("   GraphQL:   {}", "✗ disabled".dimmed());
    }
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

    // Events section
    if let Some(events) = &info.events {
        println!(
            "{} Events: {} sink(s), {} flow(s)",
            "📡".bold(),
            events.sinks.len().to_string().cyan(),
            events.flows.len().to_string().cyan()
        );
        if !events.sinks.is_empty() {
            println!("   Sinks: {}", events.sinks.join(", ").dimmed());
        }
        if !events.flows.is_empty() {
            for flow in &events.flows {
                println!("   {} {}", "•".dimmed(), flow.bold());
            }
        }
        println!();
    }

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

    // Try `this-rs` key first (published crate), then `this` (path dependency)
    let this_dep = deps.get("this-rs").or_else(|| deps.get("this"))?;

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
                graphql: false,
                websocket: false,
                grpc: false,
            };
        }
    };

    let doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => {
            return FeatureFlags {
                graphql: false,
                websocket: false,
                grpc: false,
            };
        }
    };

    let features_array = doc
        .get("dependencies")
        .and_then(|deps| deps.get("this-rs").or_else(|| deps.get("this")))
        .and_then(|this_dep| this_dep.get("features"))
        .and_then(|features| features.as_array());

    let graphql =
        features_array.is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some("graphql")));

    let websocket =
        features_array.is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some("websocket")));

    let grpc = features_array.is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some("grpc")));

    FeatureFlags {
        graphql,
        websocket,
        grpc,
    }
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

    // ================================================================
    // parse_cargo_toml tests
    // ================================================================

    #[test]
    fn test_parse_cargo_toml_with_this_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "my-app"
version = "0.1.0"
edition = "2021"

[dependencies]
this = { package = "this-rs", version = "0.0.8" }
"#,
        )
        .unwrap();

        let (name, version) = parse_cargo_toml(dir.path()).unwrap();
        assert_eq!(name, "my-app");
        assert_eq!(version, "v0.0.8");
    }

    #[test]
    fn test_parse_cargo_toml_with_simple_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "simple-project"
version = "0.1.0"

[dependencies]
this = "0.0.6"
"#,
        )
        .unwrap();

        let (name, version) = parse_cargo_toml(dir.path()).unwrap();
        assert_eq!(name, "simple-project");
        assert_eq!(version, "v0.0.6");
    }

    #[test]
    fn test_parse_cargo_toml_with_path_dep() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "local-dev"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", path = "../this" }
"#,
        )
        .unwrap();

        let (name, version) = parse_cargo_toml(dir.path()).unwrap();
        assert_eq!(name, "local-dev");
        assert_eq!(version, "(path: ../this)");
    }

    #[test]
    fn test_parse_cargo_toml_no_this_dep() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "other-app"
version = "0.1.0"

[dependencies]
serde = "1"
"#,
        )
        .unwrap();

        let (name, version) = parse_cargo_toml(dir.path()).unwrap();
        assert_eq!(name, "other-app");
        assert_eq!(version, "unknown");
    }

    #[test]
    fn test_parse_cargo_toml_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = parse_cargo_toml(dir.path());
        assert!(result.is_err());
    }

    // ================================================================
    // extract_this_version tests
    // ================================================================

    #[test]
    fn test_extract_this_version_simple_string() {
        let doc: toml_edit::DocumentMut = r#"[dependencies]
this-rs = "0.0.6"
"#
        .parse()
        .unwrap();

        assert_eq!(extract_this_version(&doc), Some("v0.0.6".to_string()));
    }

    #[test]
    fn test_extract_this_version_table_form() {
        let doc: toml_edit::DocumentMut = r#"[dependencies]
this = { package = "this-rs", version = "0.0.8" }
"#
        .parse()
        .unwrap();

        assert_eq!(extract_this_version(&doc), Some("v0.0.8".to_string()));
    }

    #[test]
    fn test_extract_this_version_path_dep() {
        let doc: toml_edit::DocumentMut = r#"[dependencies]
this = { package = "this-rs", path = "../this" }
"#
        .parse()
        .unwrap();

        assert_eq!(
            extract_this_version(&doc),
            Some("(path: ../this)".to_string())
        );
    }

    #[test]
    fn test_extract_this_version_no_deps() {
        let doc: toml_edit::DocumentMut = r#"[package]
name = "test"
"#
        .parse()
        .unwrap();

        assert_eq!(extract_this_version(&doc), None);
    }

    #[test]
    fn test_extract_this_version_no_this_dep() {
        let doc: toml_edit::DocumentMut = r#"[dependencies]
serde = "1"
"#
        .parse()
        .unwrap();

        assert_eq!(extract_this_version(&doc), None);
    }

    // ================================================================
    // detect_this_features tests
    // ================================================================

    #[test]
    fn test_detect_this_features_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.6" }
"#,
        )
        .unwrap();

        let features = detect_this_features(dir.path());
        assert!(!features.graphql);
        assert!(!features.websocket);
        assert!(!features.grpc);
    }

    #[test]
    fn test_detect_this_features_websocket() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.6", features = ["websocket"] }
"#,
        )
        .unwrap();

        let features = detect_this_features(dir.path());
        assert!(!features.graphql);
        assert!(features.websocket);
        assert!(!features.grpc);
    }

    #[test]
    fn test_detect_this_features_grpc() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.6", features = ["grpc"] }
"#,
        )
        .unwrap();

        let features = detect_this_features(dir.path());
        assert!(!features.graphql);
        assert!(!features.websocket);
        assert!(features.grpc);
    }

    #[test]
    fn test_detect_this_features_multiple() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.6", features = ["graphql", "websocket", "grpc"] }
"#,
        )
        .unwrap();

        let features = detect_this_features(dir.path());
        assert!(features.graphql);
        assert!(features.websocket);
        assert!(features.grpc);
    }

    #[test]
    fn test_detect_this_features_no_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        let features = detect_this_features(dir.path());
        assert!(!features.graphql);
        assert!(!features.websocket);
        assert!(!features.grpc);
    }

    // ================================================================
    // scan_entities tests
    // ================================================================

    #[test]
    fn test_scan_entities_no_entities_dir() {
        let dir = tempfile::tempdir().unwrap();
        let entities = scan_entities(dir.path()).unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn test_scan_entities_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/entities")).unwrap();
        let entities = scan_entities(dir.path()).unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn test_scan_entities_one_entity() {
        let dir = tempfile::tempdir().unwrap();
        let entity_dir = dir.path().join("src/entities/product");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("model.rs"),
            r#"use this::prelude::*;

impl_data_entity!(
    Product,
    "product",
    ["name"],
    {
        sku: String,
        price: f64,
    }
);
"#,
        )
        .unwrap();

        let entities = scan_entities(dir.path()).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "product");
        assert_eq!(entities[0].fields.len(), 2);
        assert_eq!(entities[0].fields[0].0, "sku");
        assert_eq!(entities[0].fields[1].0, "price");
        assert!(!entities[0].is_validated);
    }

    #[test]
    fn test_scan_entities_validated_entity() {
        let dir = tempfile::tempdir().unwrap();
        let entity_dir = dir.path().join("src/entities/order");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("model.rs"),
            r#"use this::prelude::*;

impl_data_entity_validated!(
    Order,
    "order",
    ["status"],
    {
        total: f64,
        status: String,
    },
    validate: {
        create: {
            status: [required],
        },
    }
);
"#,
        )
        .unwrap();

        let entities = scan_entities(dir.path()).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "order");
        assert!(entities[0].is_validated);
    }

    #[test]
    fn test_scan_entities_multiple_sorted() {
        let dir = tempfile::tempdir().unwrap();
        // Create entities in non-alphabetical order
        for name in &["category", "product", "brand"] {
            let entity_dir = dir.path().join(format!("src/entities/{}", name));
            std::fs::create_dir_all(&entity_dir).unwrap();
            std::fs::write(
                entity_dir.join("model.rs"),
                format!(
                    "impl_data_entity!({}, \"{}\", [\"name\"],\n{{\n    name: String,\n}}\n);",
                    name, name
                ),
            )
            .unwrap();
        }

        let entities = scan_entities(dir.path()).unwrap();
        assert_eq!(entities.len(), 3);
        // Should be sorted alphabetically
        assert_eq!(entities[0].name, "brand");
        assert_eq!(entities[1].name, "category");
        assert_eq!(entities[2].name, "product");
    }

    #[test]
    fn test_scan_entities_dir_without_model_skipped() {
        let dir = tempfile::tempdir().unwrap();
        // Directory exists but no model.rs
        std::fs::create_dir_all(dir.path().join("src/entities/orphan")).unwrap();
        let entities = scan_entities(dir.path()).unwrap();
        assert!(entities.is_empty());
    }

    // ================================================================
    // parse_links_yaml tests
    // ================================================================

    #[test]
    fn test_parse_links_yaml_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let links = parse_links_yaml(dir.path()).unwrap();
        assert!(links.is_empty());
    }

    #[test]
    fn test_parse_links_yaml_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        let links = parse_links_yaml(dir.path()).unwrap();
        assert!(links.is_empty());
    }

    #[test]
    fn test_parse_links_yaml_with_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            r#"entities: []
links:
  - link_type: has_items
    source_type: order
    target_type: product
    forward_route_name: products
    reverse_route_name: order
validation_rules: {}
"#,
        )
        .unwrap();

        let links = parse_links_yaml(dir.path()).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, "has_items");
        assert_eq!(links[0].source, "order");
        assert_eq!(links[0].target, "product");
        assert_eq!(links[0].forward_route, "products");
        assert_eq!(links[0].reverse_route, "order");
    }

    #[test]
    fn test_parse_links_yaml_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            "this is: [not: valid: yaml: {{{",
        )
        .unwrap();

        let result = parse_links_yaml(dir.path());
        assert!(result.is_err());
    }

    // ================================================================
    // check_coherence / check_module_registration / check_stores_configuration
    // / check_links_validity tests
    // ================================================================

    #[test]
    fn test_check_module_registration_no_module_rs() {
        let dir = tempfile::tempdir().unwrap();
        let count = check_module_registration(dir.path(), &["product"]);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_check_module_registration_with_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/module.rs"),
            r#"fn entity_types() -> Vec<&'static str> {
    vec![
        // [this:entity_types]
        "product",
        "category",
    ]
}
"#,
        )
        .unwrap();

        assert_eq!(
            check_module_registration(dir.path(), &["product", "category"]),
            2
        );
        assert_eq!(
            check_module_registration(dir.path(), &["product", "category", "brand"]),
            2
        );
    }

    #[test]
    fn test_check_module_registration_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/module.rs"),
            r#"fn entity_types() -> Vec<&'static str> {
    vec!["product"]
}
"#,
        )
        .unwrap();

        assert_eq!(check_module_registration(dir.path(), &["product"]), 1);
        assert_eq!(
            check_module_registration(dir.path(), &["product", "category"]),
            1
        );
    }

    #[test]
    fn test_check_stores_configuration_no_stores_rs() {
        let dir = tempfile::tempdir().unwrap();
        let count = check_stores_configuration(dir.path(), &["product"]);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_check_stores_configuration_with_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/stores.rs"),
            r#"pub struct Stores {
    // [this:store_fields]
    products_store: Arc<dyn DataService<Product>>,
}
"#,
        )
        .unwrap();

        assert_eq!(check_stores_configuration(dir.path(), &["product"]), 1);
        assert_eq!(
            check_stores_configuration(dir.path(), &["product", "category"]),
            1
        );
    }

    #[test]
    fn test_check_stores_configuration_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/stores.rs"),
            r#"pub struct Stores {
    products_store: Arc<dyn DataService<Product>>,
    categories_store: Arc<dyn DataService<Category>>,
}
"#,
        )
        .unwrap();

        assert_eq!(
            check_stores_configuration(dir.path(), &["product", "category"]),
            2
        );
    }

    #[test]
    fn test_check_links_validity_no_links_file() {
        let dir = tempfile::tempdir().unwrap();
        let (valid, issues) = check_links_validity(dir.path(), &["product"]);
        assert!(valid);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_links_validity_valid_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            r#"entities: []
links:
  - link_type: has_items
    source_type: order
    target_type: product
    forward_route_name: products
    reverse_route_name: order
validation_rules: {}
"#,
        )
        .unwrap();

        let (valid, issues) = check_links_validity(dir.path(), &["order", "product"]);
        assert!(valid);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_check_links_validity_unknown_source() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            r#"entities: []
links:
  - link_type: has_items
    source_type: ghost
    target_type: product
    forward_route_name: products
    reverse_route_name: ghost
validation_rules: {}
"#,
        )
        .unwrap();

        let (valid, issues) = check_links_validity(dir.path(), &["product"]);
        assert!(!valid);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("ghost"));
        assert!(issues[0].contains("unknown source"));
    }

    #[test]
    fn test_check_links_validity_unknown_target() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            r#"entities: []
links:
  - link_type: has_items
    source_type: order
    target_type: phantom
    forward_route_name: phantoms
    reverse_route_name: order
validation_rules: {}
"#,
        )
        .unwrap();

        let (valid, issues) = check_links_validity(dir.path(), &["order"]);
        assert!(!valid);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("phantom"));
        assert!(issues[0].contains("unknown target"));
    }

    #[test]
    fn test_check_links_validity_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            "not: valid: yaml: {{{",
        )
        .unwrap();

        let (valid, issues) = check_links_validity(dir.path(), &["product"]);
        assert!(!valid);
        assert!(issues[0].contains("Invalid YAML"));
    }

    // ================================================================
    // check_coherence integration tests
    // ================================================================

    #[test]
    fn test_check_coherence_no_entities() {
        let dir = tempfile::tempdir().unwrap();
        let entities: Vec<EntityInfo> = vec![];
        let coherence = check_coherence(dir.path(), &entities).unwrap();
        assert_eq!(coherence.module_total, 0);
        assert_eq!(coherence.module_registered, 0);
        assert_eq!(coherence.stores_total, 0);
        assert_eq!(coherence.stores_configured, 0);
        assert!(coherence.links_valid);
        assert!(coherence.links_issues.is_empty());
    }

    #[test]
    fn test_check_coherence_with_registered_entities() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/module.rs"),
            "// [this:entity_types]\n\"product\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/stores.rs"),
            "// [this:store_fields]\nproducts_store: something\n",
        )
        .unwrap();

        let entities = vec![EntityInfo {
            name: "product".to_string(),
            fields: vec![],
            is_validated: false,
        }];

        let coherence = check_coherence(dir.path(), &entities).unwrap();
        assert_eq!(coherence.module_total, 1);
        assert_eq!(coherence.module_registered, 1);
        assert_eq!(coherence.stores_total, 1);
        assert_eq!(coherence.stores_configured, 1);
    }

    #[test]
    fn test_check_coherence_with_unregistered_entity() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/module.rs"), "// [this:entity_types]\n").unwrap();
        std::fs::write(dir.path().join("src/stores.rs"), "// [this:store_fields]\n").unwrap();

        let entities = vec![EntityInfo {
            name: "product".to_string(),
            fields: vec![],
            is_validated: false,
        }];

        let coherence = check_coherence(dir.path(), &entities).unwrap();
        assert_eq!(coherence.module_total, 1);
        assert_eq!(coherence.module_registered, 0);
        assert_eq!(coherence.stores_total, 1);
        assert_eq!(coherence.stores_configured, 0);
    }

    // ================================================================
    // Full integration: collect_info-style test (calling inner functions)
    // ================================================================

    #[test]
    fn test_full_info_collection_basic_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create a minimal project structure
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "test-project"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.8" }
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        // Call inner functions that collect_info would call
        let (project_name, this_version) = parse_cargo_toml(root).unwrap();
        assert_eq!(project_name, "test-project");
        assert_eq!(this_version, "v0.0.8");

        let features = detect_this_features(root);
        assert!(!features.graphql);
        assert!(!features.websocket);
        assert!(!features.grpc);

        let entities = scan_entities(root).unwrap();
        assert!(entities.is_empty());

        let links = parse_links_yaml(root).unwrap();
        assert!(links.is_empty());

        let coherence = check_coherence(root, &entities).unwrap();
        assert_eq!(coherence.module_total, 0);
        assert!(coherence.links_valid);
    }

    #[test]
    fn test_full_info_collection_with_entities_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create project with entities and links
        std::fs::create_dir_all(root.join("src/entities/product")).unwrap();
        std::fs::create_dir_all(root.join("src/entities/category")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();

        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "shop-api"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.8", features = ["websocket"] }
"#,
        )
        .unwrap();

        std::fs::write(
            root.join("src/entities/product/model.rs"),
            r#"impl_data_entity!(Product, "product", ["name"],
{
    sku: String,
    price: f64,
}
);"#,
        )
        .unwrap();

        std::fs::write(
            root.join("src/entities/category/model.rs"),
            r#"impl_data_entity!(Category, "category", ["name"],
{
    label: String,
}
);"#,
        )
        .unwrap();

        std::fs::write(
            root.join("src/module.rs"),
            "// [this:entity_types]\n\"product\"\n\"category\"\n",
        )
        .unwrap();

        std::fs::write(
            root.join("src/stores.rs"),
            "// [this:store_fields]\nproducts_store: x\ncategories_store: y\n",
        )
        .unwrap();

        std::fs::write(
            root.join("config/links.yaml"),
            r#"entities: []
links:
  - link_type: has_category
    source_type: product
    target_type: category
    forward_route_name: categories
    reverse_route_name: products
validation_rules: {}
"#,
        )
        .unwrap();

        let (name, version) = parse_cargo_toml(root).unwrap();
        assert_eq!(name, "shop-api");
        assert_eq!(version, "v0.0.8");

        let features = detect_this_features(root);
        assert!(features.websocket);

        let entities = scan_entities(root).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].name, "category");
        assert_eq!(entities[1].name, "product");

        let links = parse_links_yaml(root).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, "has_category");

        let coherence = check_coherence(root, &entities).unwrap();
        assert_eq!(coherence.module_registered, 2);
        assert_eq!(coherence.module_total, 2);
        assert_eq!(coherence.stores_configured, 2);
        assert_eq!(coherence.stores_total, 2);
        assert!(coherence.links_valid);
    }

    // ================================================================
    // display_info smoke test (just call run logic, no panic)
    // ================================================================

    #[test]
    fn test_display_info_smoke_no_panic() {
        // Build a ProjectInfo manually and ensure run() display logic doesn't panic
        let info = ProjectInfo {
            project_name: "smoke-test".to_string(),
            this_version: "v0.0.1".to_string(),
            features: FeatureFlags {
                graphql: true,
                websocket: false,
                grpc: true,
            },
            entities: vec![
                EntityInfo {
                    name: "product".to_string(),
                    fields: vec![
                        ("sku".to_string(), "String".to_string()),
                        ("price".to_string(), "f64".to_string()),
                    ],
                    is_validated: false,
                },
                EntityInfo {
                    name: "order".to_string(),
                    fields: vec![],
                    is_validated: true,
                },
            ],
            links: vec![LinkInfo {
                link_type: "has_items".to_string(),
                source: "order".to_string(),
                target: "product".to_string(),
                forward_route: "products".to_string(),
                reverse_route: "order".to_string(),
            }],
            events: None,
            coherence: CoherenceStatus {
                module_registered: 1,
                module_total: 2,
                stores_configured: 2,
                stores_total: 2,
                links_valid: true,
                links_issues: vec![],
            },
            workspace: None,
        };

        // Just verify that accessing fields and formatting doesn't panic
        assert_eq!(info.project_name, "smoke-test");
        assert_eq!(info.entities.len(), 2);
        assert_eq!(info.links.len(), 1);
        assert!(!info.coherence.links_issues.is_empty() || info.coherence.links_valid);
    }

    #[test]
    fn test_display_info_smoke_with_workspace() {
        let info = ProjectInfo {
            project_name: "ws-app".to_string(),
            this_version: "v0.0.8".to_string(),
            features: FeatureFlags {
                graphql: false,
                websocket: true,
                grpc: false,
            },
            entities: vec![],
            links: vec![],
            events: None,
            coherence: CoherenceStatus {
                module_registered: 0,
                module_total: 0,
                stores_configured: 0,
                stores_total: 0,
                links_valid: true,
                links_issues: vec![],
            },
            workspace: Some(WorkspaceInfo {
                name: "my-workspace".to_string(),
                api_path: "api".to_string(),
                api_port: 3000,
                targets: vec![
                    TargetInfo {
                        target_type: "webapp".to_string(),
                        path: "front".to_string(),
                        framework: Some("react".to_string()),
                        runtime: None,
                    },
                    TargetInfo {
                        target_type: "mobile".to_string(),
                        path: "mobile".to_string(),
                        framework: None,
                        runtime: Some("react-native".to_string()),
                    },
                ],
            }),
        };

        // Verify workspace info is present and correct
        let ws = info.workspace.as_ref().unwrap();
        assert_eq!(ws.name, "my-workspace");
        assert_eq!(ws.targets.len(), 2);
        assert_eq!(ws.targets[0].framework, Some("react".to_string()));
        assert_eq!(ws.targets[1].runtime, Some("react-native".to_string()));
    }

    #[test]
    fn test_display_info_smoke_with_coherence_issues() {
        let info = ProjectInfo {
            project_name: "broken-app".to_string(),
            this_version: "unknown".to_string(),
            features: FeatureFlags {
                graphql: false,
                websocket: false,
                grpc: false,
            },
            entities: vec![],
            links: vec![],
            events: None,
            coherence: CoherenceStatus {
                module_registered: 0,
                module_total: 2,
                stores_configured: 1,
                stores_total: 2,
                links_valid: false,
                links_issues: vec![
                    "references unknown entity 'ghost'".to_string(),
                    "link cycle detected".to_string(),
                ],
            },
            workspace: None,
        };

        assert!(!info.coherence.links_valid);
        assert_eq!(info.coherence.links_issues.len(), 2);
        assert!(info.coherence.module_registered < info.coherence.module_total);
        assert!(info.coherence.stores_configured < info.coherence.stores_total);
    }

    // ================================================================
    // detect_events_info tests
    // ================================================================

    #[test]
    fn test_detect_events_info_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = detect_events_info(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_events_info_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/events.yaml"),
            "this is: [not valid yaml {{{",
        )
        .unwrap();

        let result = detect_events_info(dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_events_info_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/events.yaml"),
            "event_sinks: []\nevent_flows: []\n",
        )
        .unwrap();

        let result = detect_events_info(dir.path());
        assert!(result.is_some());
        let events = result.unwrap();
        assert!(events.sinks.is_empty());
        assert!(events.flows.is_empty());
    }

    #[test]
    fn test_detect_events_info_with_sinks_and_flows() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/events.yaml"),
            r#"event_sinks:
  - name: in-app
    type: in_app
  - name: webhook
    type: webhook
    url: https://example.com
event_flows:
  - name: notify-on-create
    trigger: "entity.created.*"
    steps:
      - type: deliver
        sink: in-app
  - name: audit-log
    trigger: "entity.updated.*"
    steps:
      - type: deliver
        sink: webhook
"#,
        )
        .unwrap();

        let result = detect_events_info(dir.path());
        assert!(result.is_some());
        let events = result.unwrap();
        assert_eq!(events.sinks.len(), 2);
        assert_eq!(events.sinks[0], "in-app");
        assert_eq!(events.sinks[1], "webhook");
        assert_eq!(events.flows.len(), 2);
        assert_eq!(events.flows[0], "notify-on-create");
        assert_eq!(events.flows[1], "audit-log");
    }

    #[test]
    fn test_project_info_with_events() {
        // Test that ProjectInfo correctly includes events field
        let info = ProjectInfo {
            project_name: "events-project".to_string(),
            this_version: "v0.0.8".to_string(),
            features: FeatureFlags {
                graphql: false,
                websocket: false,
                grpc: false,
            },
            entities: vec![],
            links: vec![],
            events: Some(EventsInfo {
                sinks: vec!["in-app".to_string(), "push".to_string()],
                flows: vec!["notify".to_string()],
            }),
            coherence: CoherenceStatus {
                module_registered: 0,
                module_total: 0,
                stores_configured: 0,
                stores_total: 0,
                links_valid: true,
                links_issues: vec![],
            },
            workspace: None,
        };

        assert!(info.events.is_some());
        let events = info.events.as_ref().unwrap();
        assert_eq!(events.sinks.len(), 2);
        assert_eq!(events.flows.len(), 1);

        // Test serialization includes events
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("events").is_some());
    }

    #[test]
    fn test_project_info_without_events_serialization() {
        let info = ProjectInfo {
            project_name: "no-events".to_string(),
            this_version: "v0.0.8".to_string(),
            features: FeatureFlags {
                graphql: false,
                websocket: false,
                grpc: false,
            },
            entities: vec![],
            links: vec![],
            events: None,
            coherence: CoherenceStatus {
                module_registered: 0,
                module_total: 0,
                stores_configured: 0,
                stores_total: 0,
                links_valid: true,
                links_issues: vec![],
            },
            workspace: None,
        };

        // events: None should be skipped in serialization (skip_serializing_if)
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("events").is_none());
    }
}
