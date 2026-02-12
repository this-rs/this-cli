use std::path::Path;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use crate::utils::{markers, naming, project};

/// Result of a single diagnostic check
#[derive(Debug)]
enum DiagnosticLevel {
    Pass,
    Warn,
    Error,
}

/// Serializable diagnostic result for MCP output
#[derive(Debug, Serialize)]
pub struct SerializableDiagnostic {
    pub level: String,
    pub category: String,
    pub message: String,
}

#[derive(Debug)]
struct DiagnosticResult {
    level: DiagnosticLevel,
    category: String,
    message: String,
}

impl DiagnosticResult {
    fn pass(category: &str, message: &str) -> Self {
        Self {
            level: DiagnosticLevel::Pass,
            category: category.to_string(),
            message: message.to_string(),
        }
    }

    fn warn(category: &str, message: &str) -> Self {
        Self {
            level: DiagnosticLevel::Warn,
            category: category.to_string(),
            message: message.to_string(),
        }
    }

    fn error(category: &str, message: &str) -> Self {
        Self {
            level: DiagnosticLevel::Error,
            category: category.to_string(),
            message: message.to_string(),
        }
    }

    fn icon(&self) -> &str {
        match self.level {
            DiagnosticLevel::Pass => "✅",
            DiagnosticLevel::Warn => "⚠️",
            DiagnosticLevel::Error => "❌",
        }
    }

    fn display(&self) {
        let msg = match self.level {
            DiagnosticLevel::Pass => self.message.clone(),
            DiagnosticLevel::Warn => self.message.yellow().to_string(),
            DiagnosticLevel::Error => self.message.red().to_string(),
        };
        println!("  {} {} — {}", self.icon(), self.category.bold(), msg);
    }

    fn level_str(&self) -> &str {
        match self.level {
            DiagnosticLevel::Pass => "pass",
            DiagnosticLevel::Warn => "warn",
            DiagnosticLevel::Error => "error",
        }
    }

    fn to_serializable(&self) -> SerializableDiagnostic {
        SerializableDiagnostic {
            level: self.level_str().to_string(),
            category: self.category.clone(),
            message: self.message.clone(),
        }
    }
}

/// Collect diagnostics as structured data for MCP JSON serialization.
pub fn collect_diagnostics() -> Result<Vec<SerializableDiagnostic>> {
    let project_root = project::detect_project_root()?;
    let results = run_checks(&project_root);
    Ok(results.iter().map(|r| r.to_serializable()).collect())
}

/// Run all diagnostic checks and return results
fn run_checks(project_root: &Path) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();
    results.push(check_cargo_toml(project_root));
    results.extend(check_entities(project_root));
    results.extend(check_module_registration(project_root));
    results.extend(check_stores_configuration(project_root));
    results.extend(check_links(project_root));
    results
}

pub fn run() -> Result<()> {
    let project_root = project::detect_project_root()?;

    let project_name = detect_project_name(&project_root);
    println!();
    println!(
        "{} Checking project: {}",
        "🔍".bold(),
        project_name.cyan().bold()
    );
    println!();

    let results = run_checks(&project_root);

    // Display results
    for result in &results {
        result.display();
    }

    // Summary
    let passed = results
        .iter()
        .filter(|r| matches!(r.level, DiagnosticLevel::Pass))
        .count();
    let warnings = results
        .iter()
        .filter(|r| matches!(r.level, DiagnosticLevel::Warn))
        .count();
    let errors = results
        .iter()
        .filter(|r| matches!(r.level, DiagnosticLevel::Error))
        .count();

    println!();
    print!("Summary: ");
    print!("{}", format!("{} passed", passed).green());
    if warnings > 0 {
        print!(", {}", format!("{} warning(s)", warnings).yellow());
    }
    if errors > 0 {
        print!(", {}", format!("{} error(s)", errors).red());
    }
    println!();
    println!();

    if errors > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Detect project name from Cargo.toml
fn detect_project_name(project_root: &Path) -> String {
    let cargo_path = project_root.join("Cargo.toml");
    if let Ok(content) = std::fs::read_to_string(&cargo_path)
        && let Ok(doc) = content.parse::<toml_edit::DocumentMut>()
        && let Some(name) = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
    {
        return name.to_string();
    }
    "unknown".to_string()
}

/// Check Cargo.toml has this-rs dependency
fn check_cargo_toml(project_root: &Path) -> DiagnosticResult {
    let cargo_path = project_root.join("Cargo.toml");
    if !cargo_path.exists() {
        return DiagnosticResult::error("Cargo.toml", "File not found");
    }

    let content = match std::fs::read_to_string(&cargo_path) {
        Ok(c) => c,
        Err(e) => return DiagnosticResult::error("Cargo.toml", &format!("Cannot read: {}", e)),
    };

    let doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(e) => return DiagnosticResult::error("Cargo.toml", &format!("Invalid TOML: {}", e)),
    };

    let deps = match doc.get("dependencies") {
        Some(d) => d,
        None => return DiagnosticResult::error("Cargo.toml", "No [dependencies] section"),
    };

    match deps.get("this") {
        Some(this_dep) => {
            let version = this_dep
                .as_str()
                .map(|v| format!("v{}", v))
                .or_else(|| {
                    this_dep
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| format!("v{}", v))
                })
                .or_else(|| {
                    this_dep
                        .get("path")
                        .and_then(|v| v.as_str())
                        .map(|p| format!("path: {}", p))
                })
                .unwrap_or_else(|| "unknown version".to_string());
            DiagnosticResult::pass("Cargo.toml", &format!("this-rs {} detected", version))
        }
        None => DiagnosticResult::error(
            "Cargo.toml",
            "No 'this' dependency found. Is this a this-rs project?",
        ),
    }
}

/// Check entities: compare src/entities/ directories vs entities/mod.rs declarations
fn check_entities(project_root: &Path) -> Vec<DiagnosticResult> {
    let entities_dir = project_root.join("src/entities");
    let entities_mod_path = project_root.join("src/entities/mod.rs");

    if !entities_dir.exists() {
        return vec![DiagnosticResult::pass(
            "Entities",
            "No src/entities/ directory (no entities yet)",
        )];
    }

    // Scan directories
    let entity_dirs: Vec<String> = match std::fs::read_dir(&entities_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
        Err(_) => {
            return vec![DiagnosticResult::error(
                "Entities",
                "Cannot read src/entities/ directory",
            )];
        }
    };

    if entity_dirs.is_empty() {
        return vec![DiagnosticResult::pass(
            "Entities",
            "No entity directories found",
        )];
    }

    // Read mod.rs declarations
    let mod_declarations: Vec<String> = if entities_mod_path.exists() {
        match std::fs::read_to_string(&entities_mod_path) {
            Ok(content) => content
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    if trimmed.starts_with("pub mod ") {
                        Some(
                            trimmed
                                .trim_start_matches("pub mod ")
                                .trim_end_matches(';')
                                .to_string(),
                        )
                    } else {
                        None
                    }
                })
                .collect(),
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    let mut results = Vec::new();

    // Check for orphan directories (in filesystem but not in mod.rs)
    let orphans: Vec<&String> = entity_dirs
        .iter()
        .filter(|d| !mod_declarations.contains(d))
        .collect();

    // Check for missing directories (in mod.rs but not in filesystem)
    let missing: Vec<&String> = mod_declarations
        .iter()
        .filter(|m| !entity_dirs.contains(m))
        .collect();

    if orphans.is_empty() && missing.is_empty() {
        results.push(DiagnosticResult::pass(
            "Entities",
            &format!(
                "{} entities found, all declared in mod.rs",
                entity_dirs.len()
            ),
        ));
    } else {
        if !orphans.is_empty() {
            for orphan in &orphans {
                results.push(DiagnosticResult::warn(
                    "Entities",
                    &format!(
                        "Directory src/entities/{} exists but not declared in mod.rs",
                        orphan
                    ),
                ));
            }
        }
        if !missing.is_empty() {
            for m in &missing {
                results.push(DiagnosticResult::error(
                    "Entities",
                    &format!("mod.rs declares 'pub mod {}' but directory not found", m),
                ));
            }
        }
    }

    results
}

/// Check module.rs registration: entities in mod.rs should be in module.rs entity_types
fn check_module_registration(project_root: &Path) -> Vec<DiagnosticResult> {
    let module_path = project_root.join("src/module.rs");
    let entities_mod_path = project_root.join("src/entities/mod.rs");

    if !module_path.exists() {
        return vec![DiagnosticResult::warn("Module", "src/module.rs not found")];
    }

    if !entities_mod_path.exists() {
        return vec![DiagnosticResult::pass("Module", "No entities to register")];
    }

    let module_content = match std::fs::read_to_string(&module_path) {
        Ok(c) => c,
        Err(_) => {
            return vec![DiagnosticResult::error(
                "Module",
                "Cannot read src/module.rs",
            )];
        }
    };

    let entity_names: Vec<String> = match std::fs::read_to_string(&entities_mod_path) {
        Ok(content) => content
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("pub mod ") {
                    Some(
                        trimmed
                            .trim_start_matches("pub mod ")
                            .trim_end_matches(';')
                            .to_string(),
                    )
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return vec![],
    };

    if entity_names.is_empty() {
        return vec![DiagnosticResult::pass("Module", "No entities to register")];
    }

    let mut registered = 0;
    let mut unregistered = Vec::new();

    for name in &entity_names {
        let needle = format!("\"{}\"", name);
        let is_registered = if module_content.contains("[this:entity_types]") {
            markers::has_line_after_marker(&module_content, "[this:entity_types]", &needle)
        } else {
            module_content.contains(&needle)
        };

        if is_registered {
            registered += 1;
        } else {
            unregistered.push(name.clone());
        }
    }

    let mut results = Vec::new();

    if unregistered.is_empty() {
        results.push(DiagnosticResult::pass(
            "Module",
            &format!("All {} entities registered", registered),
        ));
    } else {
        for name in &unregistered {
            results.push(DiagnosticResult::warn(
                "Module",
                &format!("Entity '{}' not registered in module.rs entity_types", name),
            ));
        }
    }

    results
}

/// Check stores.rs configuration
fn check_stores_configuration(project_root: &Path) -> Vec<DiagnosticResult> {
    let stores_path = project_root.join("src/stores.rs");
    let entities_mod_path = project_root.join("src/entities/mod.rs");

    if !stores_path.exists() {
        return vec![DiagnosticResult::warn("Stores", "src/stores.rs not found")];
    }

    if !entities_mod_path.exists() {
        return vec![DiagnosticResult::pass("Stores", "No stores to configure")];
    }

    let stores_content = match std::fs::read_to_string(&stores_path) {
        Ok(c) => c,
        Err(_) => {
            return vec![DiagnosticResult::error(
                "Stores",
                "Cannot read src/stores.rs",
            )];
        }
    };

    let entity_names: Vec<String> = match std::fs::read_to_string(&entities_mod_path) {
        Ok(content) => content
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.starts_with("pub mod ") {
                    Some(
                        trimmed
                            .trim_start_matches("pub mod ")
                            .trim_end_matches(';')
                            .to_string(),
                    )
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => return vec![],
    };

    if entity_names.is_empty() {
        return vec![DiagnosticResult::pass("Stores", "No stores to configure")];
    }

    let mut configured = 0;
    let mut missing = Vec::new();

    for name in &entity_names {
        let plural = naming::pluralize(name);
        let needle = format!("{}_store:", plural);
        let is_configured = if stores_content.contains("[this:store_fields]") {
            markers::has_line_after_marker(&stores_content, "[this:store_fields]", &needle)
        } else {
            stores_content.contains(&needle)
        };

        if is_configured {
            configured += 1;
        } else {
            missing.push(name.clone());
        }
    }

    let mut results = Vec::new();

    if missing.is_empty() {
        results.push(DiagnosticResult::pass(
            "Stores",
            &format!("All {} stores configured", configured),
        ));
    } else {
        for name in &missing {
            results.push(DiagnosticResult::warn(
                "Stores",
                &format!("No store configured for entity '{}'", name),
            ));
        }
    }

    results
}

/// Check links.yaml validity
fn check_links(project_root: &Path) -> Vec<DiagnosticResult> {
    let links_path = project_root.join("config/links.yaml");

    if !links_path.exists() {
        return vec![DiagnosticResult::warn(
            "Links",
            "config/links.yaml not found",
        )];
    }

    let content = match std::fs::read_to_string(&links_path) {
        Ok(c) => c,
        Err(e) => {
            return vec![DiagnosticResult::error(
                "Links",
                &format!("Cannot read config/links.yaml: {}", e),
            )];
        }
    };

    let config: super::add_link::LinksConfig = match serde_yaml::from_str(&content) {
        Ok(c) => c,
        Err(e) => {
            return vec![DiagnosticResult::error(
                "Links",
                &format!("Invalid YAML: {}", e),
            )];
        }
    };

    if config.links.is_empty() {
        return vec![DiagnosticResult::pass(
            "Links",
            "No links configured (empty)",
        )];
    }

    // Collect known entity names from entities dir
    let entities_dir = project_root.join("src/entities");
    let known_entities: Vec<String> = if entities_dir.exists() {
        std::fs::read_dir(&entities_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        vec![]
    };

    // Also include entities from links.yaml entities section
    let yaml_entities: Vec<String> = config.entities.iter().map(|e| e.singular.clone()).collect();

    let mut results = Vec::new();
    let mut has_issues = false;

    for link in &config.links {
        let source_known =
            known_entities.contains(&link.source_type) || yaml_entities.contains(&link.source_type);
        let target_known =
            known_entities.contains(&link.target_type) || yaml_entities.contains(&link.target_type);

        if !source_known {
            results.push(DiagnosticResult::warn(
                "Links",
                &format!(
                    "'{}' references unknown source entity '{}'",
                    link.link_type, link.source_type
                ),
            ));
            has_issues = true;
        }
        if !target_known {
            results.push(DiagnosticResult::warn(
                "Links",
                &format!(
                    "'{}' references unknown target entity '{}'",
                    link.link_type, link.target_type
                ),
            ));
            has_issues = true;
        }
    }

    if !has_issues {
        results.push(DiagnosticResult::pass(
            "Links",
            &format!("Valid configuration ({} links)", config.links.len()),
        ));
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_cargo_toml_with_this() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2024"

[dependencies]
this = { package = "this-rs", version = "0.0.6" }
"#,
        )
        .unwrap();

        let result = check_cargo_toml(dir.path());
        assert!(matches!(result.level, DiagnosticLevel::Pass));
        assert!(result.message.contains("v0.0.6"));
    }

    #[test]
    fn test_check_cargo_toml_without_this() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test-project"
version = "0.1.0"

[dependencies]
serde = "1"
"#,
        )
        .unwrap();

        let result = check_cargo_toml(dir.path());
        assert!(matches!(result.level, DiagnosticLevel::Error));
    }

    #[test]
    fn test_check_entities_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let results = check_entities(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
    }

    #[test]
    fn test_check_entities_orphan() {
        let dir = tempfile::tempdir().unwrap();
        let entities = dir.path().join("src/entities");
        std::fs::create_dir_all(entities.join("product")).unwrap();
        std::fs::write(entities.join("mod.rs"), "").unwrap();

        let results = check_entities(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Warn) && r.message.contains("product"))
        );
    }

    #[test]
    fn test_check_entities_all_declared() {
        let dir = tempfile::tempdir().unwrap();
        let entities = dir.path().join("src/entities");
        std::fs::create_dir_all(entities.join("product")).unwrap();
        std::fs::write(entities.join("mod.rs"), "pub mod product;\n").unwrap();

        let results = check_entities(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("1 entities"));
    }

    #[test]
    fn test_check_links_invalid_entity() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            r#"entities: []
links:
  - link_type: has_ghost
    source_type: product
    target_type: ghost
    forward_route_name: ghosts
    reverse_route_name: product
validation_rules: {}
"#,
        )
        .unwrap();

        let results = check_links(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Warn) && r.message.contains("ghost"))
        );
    }
}
