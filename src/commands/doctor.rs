use std::path::Path;

use anyhow::Result;
use colored::Colorize;
use serde::Serialize;

use crate::config;
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

    // Workspace checks (only if inside a workspace)
    if let Some(ws_root) = project::find_workspace_root() {
        results.extend(check_workspace(&ws_root));
    }

    results.push(check_cargo_toml(project_root));
    results.extend(check_entities(project_root));
    results.extend(check_module_registration(project_root));
    results.extend(check_stores_configuration(project_root));
    results.extend(check_links(project_root));
    results.extend(check_websocket(project_root));
    results.extend(check_grpc(project_root));
    results.extend(check_events(project_root));
    results.extend(check_policies(project_root));
    results.extend(check_plurals(
        project_root,
        crate::descriptor::enabled(project_root),
    ));
    results.extend(check_descriptor(project_root));
    results
}

pub fn run() -> Result<()> {
    let project_root = project::detect_project_root()?;

    let project_name = detect_project_name(&project_root);
    println!();
    if project::find_workspace_root().is_some() {
        println!(
            "{} Checking workspace project: {}",
            "🔍".bold(),
            project_name.cyan().bold()
        );
    } else {
        println!(
            "{} Checking project: {}",
            "🔍".bold(),
            project_name.cyan().bold()
        );
    }
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

/// Check workspace integrity: this.yaml parsable, api/ exists, target dirs present
fn check_workspace(ws_root: &Path) -> Vec<DiagnosticResult> {
    let mut results = Vec::new();
    let this_yaml_path = ws_root.join("this.yaml");

    // Check this.yaml is parsable
    let ws_config = match config::load_workspace_config(&this_yaml_path) {
        Ok(config) => {
            results.push(DiagnosticResult::pass(
                "Workspace",
                &format!("this.yaml valid (workspace: {})", config.name),
            ));
            config
        }
        Err(e) => {
            results.push(DiagnosticResult::error(
                "Workspace",
                &format!("this.yaml invalid: {}", e),
            ));
            return results;
        }
    };

    // Check api/ directory exists with Cargo.toml
    let api_dir = ws_root.join(&ws_config.api.path);
    if api_dir.join("Cargo.toml").exists() {
        results.push(DiagnosticResult::pass(
            "Workspace",
            &format!("{}/Cargo.toml found", ws_config.api.path),
        ));
    } else {
        results.push(DiagnosticResult::error(
            "Workspace",
            &format!(
                "{}/Cargo.toml not found — API directory missing or incomplete",
                ws_config.api.path
            ),
        ));
    }

    // Check each declared target has its directory
    for target in &ws_config.targets {
        let target_dir = ws_root.join(&target.path);
        if target_dir.exists() {
            results.push(DiagnosticResult::pass(
                "Workspace",
                &format!("Target {} → {} exists", target.target_type, target.path),
            ));
        } else {
            results.push(DiagnosticResult::warn(
                "Workspace",
                &format!(
                    "Target {} declared but directory {} not found",
                    target.target_type, target.path
                ),
            ));
        }
    }

    results
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

// ---------------------------------------------------------------------------
// Module descriptor checks
// ---------------------------------------------------------------------------

/// Policy strings of links.yaml (`entities[].auth.*`, `links[].auth.*`), with their path.
fn links_yaml_policies(config: &super::add_link::LinksConfig) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let auth_pairs = |a: &super::add_link::EntityAuth| {
        let mut v = vec![
            ("list".to_string(), a.list.clone()),
            ("get".to_string(), a.get.clone()),
            ("create".to_string(), a.create.clone()),
            ("update".to_string(), a.update.clone()),
            ("delete".to_string(), a.delete.clone()),
        ];
        v.extend(
            a.extra
                .iter()
                .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_string()))),
        );
        v
    };
    for e in &config.entities {
        for (op, p) in auth_pairs(&e.auth) {
            out.push((format!("entities.{}.auth.{op}", e.singular), p));
        }
    }
    for l in &config.links {
        if let Some(a) = &l.auth {
            let pairs = [
                ("list", &a.list),
                ("get", &a.get),
                ("create", &a.create),
                ("update", &a.update),
                ("delete", &a.delete),
            ];
            for (op, p) in pairs {
                out.push((format!("links.{}.auth.{op}", l.link_type), p.clone()));
            }
            for (k, v) in &a.extra {
                if let Some(s) = v.as_str() {
                    out.push((format!("links.{}.auth.{k}", l.link_type), s.to_string()));
                }
            }
        }
    }
    out
}

/// Unknown policies in links.yaml: the runtime parser falls back to `authenticated`
/// (fail-open) where the descriptor AST refuses them. Reported in every project.
fn check_policies(project_root: &Path) -> Vec<DiagnosticResult> {
    let Ok(content) = std::fs::read_to_string(project_root.join("config/links.yaml")) else {
        return vec![];
    };
    let Ok(config) = serde_yaml::from_str::<super::add_link::LinksConfig>(&content) else {
        return vec![];
    };
    links_yaml_policies(&config)
        .into_iter()
        .filter_map(|(path, p)| {
            this_descriptor::PolicyExpr::parse(&p).err().map(|e| {
                DiagnosticResult::error("Policies", &format!("{path}: unknown policy '{p}' ({e})"))
            })
        })
        .collect()
}

/// Plural of each entity as declared in each place that redeclares it.
fn plural_sources(project_root: &Path) -> Vec<(String, Vec<(String, String)>)> {
    let mut by_entity: std::collections::BTreeMap<String, Vec<(String, String)>> =
        Default::default();
    for (dir, _) in crate::descriptor::model_paths(project_root) {
        let desc_rs = project_root
            .join("src/entities")
            .join(&dir)
            .join("descriptor.rs");
        if let Ok((plural, _)) = crate::codegen::introspect::parse_descriptor(&desc_rs)
            && !plural.is_empty()
        {
            by_entity
                .entry(dir.clone())
                .or_default()
                .push((format!("src/entities/{dir}/descriptor.rs"), plural));
        }
    }
    if let Ok(content) = std::fs::read_to_string(project_root.join("config/links.yaml"))
        && let Ok(config) = serde_yaml::from_str::<super::add_link::LinksConfig>(&content)
    {
        for e in config.entities {
            by_entity
                .entry(e.singular.clone())
                .or_default()
                .push(("config/links.yaml".into(), e.plural));
        }
    }
    if let Ok(Some((_, source))) = crate::descriptor::read_source(project_root) {
        for e in source.entities {
            // `plural` is filled by normalize(): only an explicit one is a declaration.
            if e.plural != this_descriptor::naming::Pluralizer::pluralize(&e.name) {
                by_entity
                    .entry(e.name.clone())
                    .or_default()
                    .push((crate::descriptor::MODULE_YAML.into(), e.plural));
            }
        }
    }
    by_entity.into_iter().collect()
}

/// Plural drift: every redeclared plural must equal the runtime one,
/// `Pluralizer::pluralize(name)` (used by the macro, `Entity::resource_name()` and the
/// descriptor). In descriptor mode the server refuses to start on a drift
/// (`this.module.plural_mismatch`): error; otherwise a warning.
fn check_plurals(project_root: &Path, descriptor_mode: bool) -> Vec<DiagnosticResult> {
    let mut out = Vec::new();
    for (entity, sources) in plural_sources(project_root) {
        let expected = this_descriptor::naming::Pluralizer::pluralize(&entity);
        for (place, plural) in sources {
            if plural != expected {
                let msg = format!(
                    "plural drift for '{entity}': {place} says '{plural}', the runtime uses '{expected}'"
                );
                out.push(if descriptor_mode {
                    DiagnosticResult::error("Plurals", &msg)
                } else {
                    DiagnosticResult::warn("Plurals", &msg)
                });
            }
        }
    }
    out
}

/// Descriptor mode only (descriptor/module.yaml exists).
fn check_descriptor(project_root: &Path) -> Vec<DiagnosticResult> {
    use crate::descriptor;
    if !descriptor::enabled(project_root) {
        return vec![];
    }
    let mut out = Vec::new();
    match descriptor::check_project(project_root) {
        Ok(Some(r)) => {
            out.extend(
                r.errors
                    .iter()
                    .map(|e| DiagnosticResult::error("Descriptor", e)),
            );
            out.extend(
                r.warnings
                    .iter()
                    .map(|w| DiagnosticResult::warn("Descriptor", w)),
            );
        }
        Ok(None) => {}
        Err(e) => out.push(DiagnosticResult::error("Descriptor", &format!("{e:#}"))),
    }
    let current = match descriptor::current(project_root) {
        Ok(Some(d)) => d,
        Ok(None) => return out,
        Err(e) => {
            out.push(DiagnosticResult::error("Descriptor", &format!("{e:#}")));
            return out;
        }
    };

    // Field drift: committed module.descriptor.json vs code.
    if let Ok(text) = std::fs::read_to_string(project_root.join(descriptor::MATERIALIZED)) {
        match this_descriptor::ModuleDescriptor::from_json(&text) {
            Ok(saved) => {
                for e in &current.entities {
                    let Some(s) = saved.entity(&e.name) else {
                        out.push(DiagnosticResult::warn(
                            "Fields",
                            &format!("'{}' missing from {}", e.name, descriptor::MATERIALIZED),
                        ));
                        continue;
                    };
                    let (a, b) = (
                        descriptor::code::canonical(e),
                        descriptor::code::canonical(s),
                    );
                    for f in &a.fields {
                        match b.field(&f.name) {
                            None => out.push(DiagnosticResult::warn(
                                "Fields",
                                &format!(
                                    "{}.{}: in the code, not in {}",
                                    e.name,
                                    f.name,
                                    descriptor::MATERIALIZED
                                ),
                            )),
                            Some(g) if g != f => out.push(DiagnosticResult::warn(
                                "Fields",
                                &format!(
                                    "{}.{}: type or rules differ from {}",
                                    e.name,
                                    f.name,
                                    descriptor::MATERIALIZED
                                ),
                            )),
                            Some(_) => {}
                        }
                    }
                    for g in &b.fields {
                        if a.field(&g.name).is_none() {
                            out.push(DiagnosticResult::warn(
                                "Fields",
                                &format!(
                                    "{}.{}: in {}, no longer in the code",
                                    e.name,
                                    g.name,
                                    descriptor::MATERIALIZED
                                ),
                            ));
                        }
                    }
                }
            }
            Err(e) => out.push(DiagnosticResult::error(
                "Fields",
                &format!(
                    "{}: {:#}",
                    descriptor::MATERIALIZED,
                    descriptor::io::contract(e)
                ),
            )),
        }
    }

    // Links to entities declared nowhere (this module or a copied required manifest).
    let required: Vec<this_descriptor::ModuleDescriptor> = current
        .requires
        .iter()
        .filter_map(|r| {
            let p = project_root.join(format!("{}/modules/{}.json", descriptor::DIR, r.name));
            std::fs::read_to_string(p)
                .ok()
                .and_then(|t| this_descriptor::ModuleDescriptor::from_json(&t).ok())
        })
        .collect();
    for l in &current.links {
        for end in [&l.source, &l.target] {
            let here = current.entity(end).is_some();
            let there = required.iter().any(|m| m.entity(end).is_some());
            if !here && !there {
                out.push(DiagnosticResult::error(
                    "Links",
                    &format!(
                        "link '{}' ({} -> {}) references '{end}', declared by no module",
                        l.link_type, l.source, l.target
                    ),
                ));
            }
        }
    }

    // Identity: what build_host compares (this.module.identity_mismatch).
    if let Ok(module_rs) = std::fs::read_to_string(project_root.join("src/module.rs")) {
        if let Some(c) = regex::Regex::new(r#"fn name\(&self\)\s*->\s*&str\s*\{\s*"([^"]+)""#)
            .unwrap()
            .captures(&module_rs)
        {
            let name = this_descriptor::naming::to_snake_identifier(&c[1]);
            if name != current.name {
                out.push(DiagnosticResult::error(
                    "Descriptor",
                    &format!(
                        "Module::name() '{}' vs descriptor name '{}'",
                        &c[1], current.name
                    ),
                ));
            }
        }
        for e in &current.entities {
            let needle = format!("\"{}\"", e.name);
            if !markers::has_line_after_marker(&module_rs, "[this:entity_types]", &needle)
                && !module_rs.contains(&format!("{needle},"))
            {
                out.push(DiagnosticResult::error(
                    "Descriptor",
                    &format!("entity '{}' missing from Module::entity_types()", e.name),
                ));
            }
        }
        // `fn descriptor()` present and listing every entity: check_project above
        // (descriptor::registration_issues).
    }

    if !out
        .iter()
        .any(|r| !matches!(r.level, DiagnosticLevel::Pass))
    {
        out.push(DiagnosticResult::pass(
            "Descriptor",
            &format!(
                "{} v{} ({} entities, {} links)",
                current.name,
                current.version,
                current.entities.len(),
                current.links.len()
            ),
        ));
    }
    out
}

/// Check WebSocket configuration coherence:
/// If the websocket feature is enabled in Cargo.toml, main.rs should use WebSocketExposure.
fn check_websocket(project_root: &Path) -> Vec<DiagnosticResult> {
    let features = super::info::detect_this_features(project_root);

    if !features.websocket {
        // WebSocket not enabled — nothing to check
        return vec![];
    }

    // WebSocket feature is enabled — verify main.rs uses WebSocketExposure
    let main_path = project_root.join("src/main.rs");
    let main_content = match std::fs::read_to_string(&main_path) {
        Ok(c) => c,
        Err(_) => {
            return vec![DiagnosticResult::warn(
                "WebSocket",
                "websocket feature enabled but src/main.rs not found",
            )];
        }
    };

    if main_content.contains("WebSocketExposure") {
        vec![DiagnosticResult::pass(
            "WebSocket",
            "Feature enabled and WebSocketExposure configured in main.rs",
        )]
    } else {
        vec![DiagnosticResult::warn(
            "WebSocket",
            "websocket feature enabled in Cargo.toml but WebSocketExposure not found in main.rs",
        )]
    }
}

/// Check gRPC configuration coherence:
/// If the grpc feature is enabled in Cargo.toml, main.rs should use GrpcExposure.
fn check_grpc(project_root: &Path) -> Vec<DiagnosticResult> {
    let features = super::info::detect_this_features(project_root);

    if !features.grpc {
        // gRPC not enabled — nothing to check
        return vec![];
    }

    // gRPC feature is enabled — verify main.rs uses GrpcExposure
    let main_path = project_root.join("src/main.rs");
    let main_content = match std::fs::read_to_string(&main_path) {
        Ok(c) => c,
        Err(_) => {
            return vec![DiagnosticResult::warn(
                "gRPC",
                "grpc feature enabled but src/main.rs not found",
            )];
        }
    };

    if main_content.contains("GrpcExposure") {
        vec![DiagnosticResult::pass(
            "gRPC",
            "Feature enabled and GrpcExposure configured in main.rs",
        )]
    } else {
        vec![DiagnosticResult::warn(
            "gRPC",
            "grpc feature enabled in Cargo.toml but GrpcExposure not found in main.rs",
        )]
    }
}

/// Check events.yaml consistency:
/// - If events.yaml exists, parse it
/// - Check that flows reference existing sinks
/// - Check for empty sinks/flows
fn check_events(project_root: &Path) -> Vec<DiagnosticResult> {
    let events_path = project_root.join("config/events.yaml");

    if !events_path.exists() {
        // No events.yaml — check if main.rs uses event_bus (would mean missing config)
        let main_path = project_root.join("src/main.rs");
        if let Ok(main_content) = std::fs::read_to_string(&main_path)
            && (main_content.contains("with_default_event_bus")
                || main_content.contains("with_event_bus"))
        {
            return vec![DiagnosticResult::warn(
                "Events",
                "main.rs uses event bus but config/events.yaml not found",
            )];
        }
        return vec![];
    }

    let mut results = Vec::new();

    let content = match std::fs::read_to_string(&events_path) {
        Ok(c) => c,
        Err(_) => {
            results.push(DiagnosticResult::error(
                "Events",
                "config/events.yaml exists but cannot be read",
            ));
            return results;
        }
    };

    let config: crate::commands::add_event_flow::EventsConfig = match serde_yaml::from_str(&content)
    {
        Ok(c) => c,
        Err(_) => {
            results.push(DiagnosticResult::error(
                "Events",
                "config/events.yaml has invalid YAML syntax",
            ));
            return results;
        }
    };

    // Check sinks
    if config.event_sinks.is_empty() {
        results.push(DiagnosticResult::warn(
            "Events",
            "No event sinks configured in events.yaml",
        ));
    }

    // Build sink name set for flow validation
    let sink_names: std::collections::HashSet<&str> =
        config.event_sinks.iter().map(|s| s.name.as_str()).collect();

    // Check flows reference valid sinks
    let mut flow_issues = Vec::new();
    for flow in &config.event_flows {
        for step in &flow.steps {
            if step.step_type == "deliver"
                && let Some(ref sink) = step.sink
                && !sink_names.contains(sink.as_str())
            {
                flow_issues.push(format!(
                    "Flow '{}' references unknown sink '{}'",
                    flow.name, sink
                ));
            }
        }
    }

    if flow_issues.is_empty() {
        results.push(DiagnosticResult::pass(
            "Events",
            &format!(
                "{} sink(s), {} flow(s) configured — all references valid",
                config.event_sinks.len(),
                config.event_flows.len()
            ),
        ));
    } else {
        for issue in &flow_issues {
            results.push(DiagnosticResult::warn("Events", issue));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Module descriptor checks ────────────────────────────────────

    fn descriptor_project(module_yaml: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::create_dir_all(p.join("src/entities/knife")).unwrap();
        std::fs::create_dir_all(p.join("config")).unwrap();
        std::fs::write(
            p.join("src/entities/knife/model.rs"),
            "use this::prelude::*;\n\nimpl_data_entity!(\n    Knife,\n    \"knife\",\n    [\"name\"],\n    {\n        blade: String,\n    }\n);\n",
        )
        .unwrap();
        std::fs::write(
            p.join("src/entities/knife/descriptor.rs"),
            "impl EntityDescriptor for KnifeDescriptor {\n    fn plural(&self) -> &str {\n        \"knifes\"\n    }\n}\n",
        )
        .unwrap();
        std::fs::write(
            p.join("config/links.yaml"),
            "entities:\n  - singular: knife\n    plural: knifes\n    auth:\n      list: publik\n      get: authenticated\n      create: role:admin\n      update: authenticated\n      delete: authenticated\nlinks: []\n",
        )
        .unwrap();
        std::fs::write(
            p.join("src/module.rs"),
            "impl Module for ShopModule {\n    fn name(&self) -> &str {\n        \"shop\"\n    }\n    fn entity_types(&self) -> Vec<&str> {\n        vec![\n            // [this:entity_types]\n            \"knife\",\n        ]\n    }\n    fn descriptor(&self) -> Option<ModuleDescriptor> {\n        None\n    }\n}\n",
        )
        .unwrap();
        if let Some(y) = module_yaml {
            std::fs::create_dir_all(p.join("descriptor")).unwrap();
            std::fs::write(p.join("descriptor/module.yaml"), y).unwrap();
        }
        dir
    }

    #[test]
    fn doctor_reports_plural_drift() {
        // Without descriptor: a warning per place that disagrees with the runtime plural.
        let dir = descriptor_project(None);
        let r = check_plurals(dir.path(), false);
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r.iter().all(|d| matches!(d.level, DiagnosticLevel::Warn)));
        assert!(r.iter().all(|d| d.message.contains("'knives'")), "{r:?}");
        assert!(r.iter().any(|d| d.message.contains("config/links.yaml")));
        assert!(r.iter().any(|d| d.message.contains("descriptor.rs")));
        // In descriptor mode the server would refuse to start: errors.
        let r = check_plurals(dir.path(), true);
        assert!(r.iter().all(|d| matches!(d.level, DiagnosticLevel::Error)));
        // An explicit wrong plural in module.yaml is a drift too.
        let dir = descriptor_project(Some(
            "format: 1\nname: shop\nversion: 1.0.0\nentities:\n  - name: knife\n    plural: knifes\n",
        ));
        let r = check_plurals(dir.path(), true);
        assert!(
            r.iter()
                .any(|d| d.message.contains("descriptor/module.yaml")),
            "{r:?}"
        );
        // Same plural everywhere: nothing to report.
        let ok = descriptor_project(None);
        std::fs::write(
            ok.path().join("config/links.yaml"),
            "entities:\n  - singular: knife\n    plural: knives\nlinks: []\n",
        )
        .unwrap();
        std::fs::write(
            ok.path().join("src/entities/knife/descriptor.rs"),
            "fn plural(&self) -> &str {\n        \"knives\"\n    }\n",
        )
        .unwrap();
        assert!(check_plurals(ok.path(), true).is_empty());
    }

    #[test]
    fn doctor_reports_unknown_policy() {
        let dir = descriptor_project(None);
        let r = check_policies(dir.path());
        assert_eq!(r.len(), 1, "{r:?}");
        assert!(matches!(r[0].level, DiagnosticLevel::Error));
        assert!(r[0].message.contains("entities.knife.auth.list"));
        assert!(r[0].message.contains("publik"));
    }

    #[test]
    fn doctor_reports_link_to_undeclared_entity() {
        let dir = descriptor_project(Some(
            "format: 1\nname: shop\nversion: 1.0.0\nentities:\n  - name: knife\nlinks:\n  - link_type: owned_by\n    source: knife\n    target: ghost\n",
        ));
        let r = check_descriptor(dir.path());
        assert!(
            r.iter().any(|d| matches!(d.level, DiagnosticLevel::Error)
                && d.category == "Links"
                && d.message.contains("'ghost'")),
            "{r:?}"
        );
    }

    #[test]
    fn doctor_reports_field_drift_and_identity() {
        let dir = descriptor_project(Some(
            "format: 1\nname: other\nversion: 1.0.0\nentities:\n  - name: knife\n",
        ));
        // Materialized JSON written when `blade` was the only field; the code then gained
        // `edge`.
        let mut saved = crate::descriptor::current(dir.path()).unwrap().unwrap();
        saved.name = "other".into();
        std::fs::write(
            dir.path().join("descriptor/module.descriptor.json"),
            crate::descriptor::materialized_json(&saved),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/entities/knife/model.rs"),
            "impl_data_entity!(Knife, \"knife\", [\"name\"], { blade: String, edge: f64, });\n",
        )
        .unwrap();
        let r = check_descriptor(dir.path());
        assert!(
            r.iter()
                .any(|d| d.category == "Fields" && d.message.contains("knife.edge")),
            "{r:?}"
        );
        assert!(
            r.iter().any(|d| matches!(d.level, DiagnosticLevel::Error)
                && d.message
                    .contains("Module::name() 'shop' vs descriptor name 'other'")),
            "{r:?}"
        );
    }

    #[test]
    fn doctor_without_descriptor_adds_no_descriptor_diagnostic() {
        let dir = descriptor_project(None);
        assert!(check_descriptor(dir.path()).is_empty());
    }

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

    #[test]
    fn test_check_websocket_not_enabled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
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

        let results = check_websocket(dir.path());
        // No check emitted when websocket is not enabled
        assert!(results.is_empty());
    }

    #[test]
    fn test_check_websocket_enabled_and_configured() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
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
        std::fs::write(
            dir.path().join("src/main.rs"),
            "use this::server::exposure::websocket::WebSocketExposure;\nfn main() {}",
        )
        .unwrap();

        let results = check_websocket(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("WebSocketExposure"));
    }

    #[test]
    fn test_check_websocket_enabled_but_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
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
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() { println!(\"no websocket\"); }",
        )
        .unwrap();

        let results = check_websocket(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("WebSocketExposure not found"));
    }

    // ================================================================
    // check_module_registration tests
    // ================================================================

    #[test]
    fn test_check_module_registration_no_module_rs() {
        let dir = tempfile::tempdir().unwrap();
        let results = check_module_registration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("module.rs not found"));
    }

    #[test]
    fn test_check_module_registration_no_entities() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/module.rs"),
            "fn entity_types() -> Vec<&'static str> { vec![] }",
        )
        .unwrap();

        let results = check_module_registration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("No entities"));
    }

    #[test]
    fn test_check_module_registration_all_registered_with_marker() {
        let dir = tempfile::tempdir().unwrap();
        let entities_dir = dir.path().join("src/entities");
        std::fs::create_dir_all(&entities_dir).unwrap();
        std::fs::write(
            entities_dir.join("mod.rs"),
            "pub mod product;\npub mod category;\n",
        )
        .unwrap();
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

        let results = check_module_registration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("All 2 entities registered"));
    }

    #[test]
    fn test_check_module_registration_missing_entity() {
        let dir = tempfile::tempdir().unwrap();
        let entities_dir = dir.path().join("src/entities");
        std::fs::create_dir_all(&entities_dir).unwrap();
        std::fs::write(
            entities_dir.join("mod.rs"),
            "pub mod product;\npub mod category;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/module.rs"),
            r#"fn entity_types() -> Vec<&'static str> {
    vec![
        // [this:entity_types]
        "product",
    ]
}
"#,
        )
        .unwrap();

        let results = check_module_registration(dir.path());
        assert!(results
            .iter()
            .any(|r| matches!(r.level, DiagnosticLevel::Warn)
                && r.message.contains("category")));
    }

    #[test]
    fn test_check_module_registration_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let entities_dir = dir.path().join("src/entities");
        std::fs::create_dir_all(&entities_dir).unwrap();
        std::fs::write(entities_dir.join("mod.rs"), "pub mod product;\n").unwrap();
        std::fs::write(
            dir.path().join("src/module.rs"),
            "fn entity_types() -> Vec<&'static str> { vec![\"product\"] }",
        )
        .unwrap();

        let results = check_module_registration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("All 1 entities registered"));
    }

    // ================================================================
    // check_stores_configuration tests
    // ================================================================

    #[test]
    fn test_check_stores_no_stores_rs() {
        let dir = tempfile::tempdir().unwrap();
        let results = check_stores_configuration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("stores.rs not found"));
    }

    #[test]
    fn test_check_stores_no_entities() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/stores.rs"), "pub struct Stores {}").unwrap();

        let results = check_stores_configuration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("No stores"));
    }

    #[test]
    fn test_check_stores_all_configured_with_marker() {
        let dir = tempfile::tempdir().unwrap();
        let entities_dir = dir.path().join("src/entities");
        std::fs::create_dir_all(&entities_dir).unwrap();
        std::fs::write(
            entities_dir.join("mod.rs"),
            "pub mod product;\npub mod category;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/stores.rs"),
            r#"pub struct Stores {
    // [this:store_fields]
    products_store: Arc<dyn DataService<Product>>,
    categories_store: Arc<dyn DataService<Category>>,
}
"#,
        )
        .unwrap();

        let results = check_stores_configuration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("All 2 stores configured"));
    }

    #[test]
    fn test_check_stores_missing_entity_store() {
        let dir = tempfile::tempdir().unwrap();
        let entities_dir = dir.path().join("src/entities");
        std::fs::create_dir_all(&entities_dir).unwrap();
        std::fs::write(
            entities_dir.join("mod.rs"),
            "pub mod product;\npub mod category;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/stores.rs"),
            r#"pub struct Stores {
    // [this:store_fields]
    products_store: Arc<dyn DataService<Product>>,
}
"#,
        )
        .unwrap();

        let results = check_stores_configuration(dir.path());
        assert!(results
            .iter()
            .any(|r| matches!(r.level, DiagnosticLevel::Warn)
                && r.message.contains("category")));
    }

    #[test]
    fn test_check_stores_without_marker() {
        let dir = tempfile::tempdir().unwrap();
        let entities_dir = dir.path().join("src/entities");
        std::fs::create_dir_all(&entities_dir).unwrap();
        std::fs::write(entities_dir.join("mod.rs"), "pub mod product;\n").unwrap();
        std::fs::write(
            dir.path().join("src/stores.rs"),
            "pub struct Stores {\n    products_store: Arc<dyn DataService<Product>>,\n}",
        )
        .unwrap();

        let results = check_stores_configuration(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
    }

    // ================================================================
    // check_grpc tests
    // ================================================================

    #[test]
    fn test_check_grpc_not_enabled() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
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

        let results = check_grpc(dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn test_check_grpc_enabled_and_configured() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
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
        std::fs::write(
            dir.path().join("src/main.rs"),
            "use this::server::exposure::grpc::GrpcExposure;\nfn main() {}",
        )
        .unwrap();

        let results = check_grpc(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("GrpcExposure"));
    }

    #[test]
    fn test_check_grpc_enabled_but_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
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
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() { println!(\"no grpc\"); }",
        )
        .unwrap();

        let results = check_grpc(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("GrpcExposure not found"));
    }

    #[test]
    fn test_check_grpc_enabled_but_no_main_rs() {
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

        let results = check_grpc(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("main.rs not found"));
    }

    // ================================================================
    // check_workspace tests
    // ================================================================

    #[test]
    fn test_check_workspace_valid() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(
            dir.path().join("api/Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("this.yaml"),
            "name: my-workspace\napi:\n  path: api\n  port: 3000\ntargets: []\n",
        )
        .unwrap();

        let results = check_workspace(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Pass)
                    && r.message.contains("this.yaml valid"))
        );
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Pass)
                    && r.message.contains("Cargo.toml found"))
        );
    }

    #[test]
    fn test_check_workspace_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("this.yaml"),
            "this is: [not: valid: yaml: {{{",
        )
        .unwrap();

        let results = check_workspace(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Error)
                    && r.message.contains("this.yaml invalid"))
        );
    }

    #[test]
    fn test_check_workspace_missing_api_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("this.yaml"),
            "name: my-workspace\napi:\n  path: api\n  port: 3000\ntargets: []\n",
        )
        .unwrap();

        let results = check_workspace(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Error)
                    && r.message.contains("not found"))
        );
    }

    #[test]
    fn test_check_workspace_with_targets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::create_dir_all(dir.path().join("front")).unwrap();
        std::fs::write(
            dir.path().join("api/Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("this.yaml"),
            r#"name: my-workspace
api:
  path: api
  port: 3000
targets:
  - target_type: webapp
    framework: react
    path: front
"#,
        )
        .unwrap();

        let results = check_workspace(dir.path());
        // Should have pass for this.yaml, api/Cargo.toml, and target
        let pass_count = results
            .iter()
            .filter(|r| matches!(r.level, DiagnosticLevel::Pass))
            .count();
        assert!(pass_count >= 3);
    }

    #[test]
    fn test_check_workspace_target_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("api")).unwrap();
        std::fs::write(
            dir.path().join("api/Cargo.toml"),
            "[package]\nname = \"test\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("this.yaml"),
            r#"name: my-workspace
api:
  path: api
  port: 3000
targets:
  - target_type: webapp
    framework: react
    path: front
"#,
        )
        .unwrap();

        let results = check_workspace(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Warn)
                    && r.message.contains("directory")
                    && r.message.contains("not found"))
        );
    }

    // ================================================================
    // check_links tests (additional)
    // ================================================================

    #[test]
    fn test_check_links_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let results = check_links(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("not found"));
    }

    #[test]
    fn test_check_links_empty_links() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        let results = check_links(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("empty"));
    }

    #[test]
    fn test_check_links_valid_with_known_entities() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/entities/product")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/entities/category")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
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

        let results = check_links(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("1 links"));
    }

    #[test]
    fn test_check_links_valid_with_yaml_entities() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            r#"entities:
  - singular: order
    plural: orders
  - singular: product
    plural: products
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

        let results = check_links(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
    }

    #[test]
    fn test_check_links_invalid_yaml_syntax() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/links.yaml"),
            "this is: [not: valid: yaml: {{{",
        )
        .unwrap();

        let results = check_links(dir.path());
        assert!(results.iter().any(
            |r| matches!(r.level, DiagnosticLevel::Error) && r.message.contains("Invalid YAML")
        ));
    }

    // ================================================================
    // check_cargo_toml tests (additional)
    // ================================================================

    #[test]
    fn test_check_cargo_toml_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = check_cargo_toml(dir.path());
        assert!(matches!(result.level, DiagnosticLevel::Error));
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_check_cargo_toml_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "this is not valid toml [[[").unwrap();

        let result = check_cargo_toml(dir.path());
        assert!(matches!(result.level, DiagnosticLevel::Error));
        assert!(result.message.contains("Invalid TOML"));
    }

    #[test]
    fn test_check_cargo_toml_no_deps_section() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let result = check_cargo_toml(dir.path());
        assert!(matches!(result.level, DiagnosticLevel::Error));
        assert!(result.message.contains("No [dependencies] section"));
    }

    #[test]
    fn test_check_cargo_toml_with_path_dep() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", path = "../this" }
"#,
        )
        .unwrap();

        let result = check_cargo_toml(dir.path());
        assert!(matches!(result.level, DiagnosticLevel::Pass));
        assert!(result.message.contains("path:"));
    }

    // ================================================================
    // run_checks integration test
    // ================================================================

    #[test]
    fn test_run_checks_healthy_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create a minimal healthy project
        std::fs::create_dir_all(root.join("src/entities/product")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();

        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "healthy-project"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.8" }
"#,
        )
        .unwrap();

        std::fs::write(root.join("src/entities/mod.rs"), "pub mod product;\n").unwrap();

        std::fs::write(
            root.join("src/module.rs"),
            "// [this:entity_types]\n\"product\"\n",
        )
        .unwrap();

        std::fs::write(
            root.join("src/stores.rs"),
            "// [this:store_fields]\nproducts_store: x\n",
        )
        .unwrap();

        std::fs::write(
            root.join("config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        let results = run_checks(root);

        // All results should be Pass
        let errors = results
            .iter()
            .filter(|r| matches!(r.level, DiagnosticLevel::Error))
            .count();
        assert_eq!(errors, 0, "Healthy project should have no errors");

        // At least the Cargo.toml, Entities, Module, Stores checks should pass
        let passes = results
            .iter()
            .filter(|r| matches!(r.level, DiagnosticLevel::Pass))
            .count();
        assert!(
            passes >= 4,
            "Expected at least 4 pass results, got {}",
            passes
        );
    }

    #[test]
    fn test_run_checks_broken_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Broken project: entity in entities dir but not registered
        std::fs::create_dir_all(root.join("src/entities/product")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();

        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "broken-project"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.8" }
"#,
        )
        .unwrap();

        // Entities mod.rs declares product but module.rs doesn't register it
        std::fs::write(root.join("src/entities/mod.rs"), "pub mod product;\n").unwrap();

        // Empty module.rs and stores.rs
        std::fs::write(root.join("src/module.rs"), "fn entity_types() {}").unwrap();
        std::fs::write(root.join("src/stores.rs"), "pub struct Stores {}").unwrap();

        // links.yaml referencing unknown entities
        std::fs::write(
            root.join("config/links.yaml"),
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

        let results = run_checks(root);

        // Should have warnings for module registration and stores
        let warnings = results
            .iter()
            .filter(|r| matches!(r.level, DiagnosticLevel::Warn))
            .count();
        assert!(
            warnings >= 2,
            "Broken project should have at least 2 warnings, got {}",
            warnings
        );
    }

    // ================================================================
    // DiagnosticResult and display tests
    // ================================================================

    #[test]
    fn test_diagnostic_result_pass() {
        let result = DiagnosticResult::pass("Test", "Everything is fine");
        assert!(matches!(result.level, DiagnosticLevel::Pass));
        assert_eq!(result.category, "Test");
        assert_eq!(result.message, "Everything is fine");
        assert_eq!(result.icon(), "\u{2705}");
        assert_eq!(result.level_str(), "pass");
    }

    #[test]
    fn test_diagnostic_result_warn() {
        let result = DiagnosticResult::warn("Test", "Something looks off");
        assert!(matches!(result.level, DiagnosticLevel::Warn));
        assert_eq!(result.icon(), "\u{26a0}\u{fe0f}");
        assert_eq!(result.level_str(), "warn");
    }

    #[test]
    fn test_diagnostic_result_error() {
        let result = DiagnosticResult::error("Test", "Something is broken");
        assert!(matches!(result.level, DiagnosticLevel::Error));
        assert_eq!(result.icon(), "\u{274c}");
        assert_eq!(result.level_str(), "error");
    }

    #[test]
    fn test_diagnostic_result_to_serializable() {
        let result = DiagnosticResult::warn("Links", "Unknown entity reference");
        let serializable = result.to_serializable();
        assert_eq!(serializable.level, "warn");
        assert_eq!(serializable.category, "Links");
        assert_eq!(serializable.message, "Unknown entity reference");
    }

    #[test]
    fn test_display_diagnostics_smoke_no_panic() {
        // Ensure calling display() on various DiagnosticResults doesn't panic
        let results = vec![
            DiagnosticResult::pass("Cargo.toml", "this-rs v0.0.8 detected"),
            DiagnosticResult::warn("Module", "Entity 'product' not registered"),
            DiagnosticResult::error("Entities", "mod.rs declares non-existent entity"),
        ];

        for result in &results {
            result.display(); // Should not panic
        }
    }

    // ================================================================
    // detect_project_name tests
    // ================================================================

    #[test]
    fn test_detect_project_name_valid() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"my-cool-app\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let name = detect_project_name(dir.path());
        assert_eq!(name, "my-cool-app");
    }

    #[test]
    fn test_detect_project_name_missing_cargo() {
        let dir = tempfile::tempdir().unwrap();
        let name = detect_project_name(dir.path());
        assert_eq!(name, "unknown");
    }

    #[test]
    fn test_detect_project_name_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "not valid toml [[[").unwrap();

        let name = detect_project_name(dir.path());
        assert_eq!(name, "unknown");
    }

    // ================================================================
    // check_entities additional tests
    // ================================================================

    #[test]
    fn test_check_entities_missing_dir_in_mod_rs() {
        let dir = tempfile::tempdir().unwrap();
        let entities = dir.path().join("src/entities");
        std::fs::create_dir_all(entities.join("product")).unwrap();
        // mod.rs declares product AND category, but category dir doesn't exist
        std::fs::write(
            entities.join("mod.rs"),
            "pub mod product;\npub mod category;\n",
        )
        .unwrap();

        let results = check_entities(dir.path());
        assert!(
            results.iter().any(
                |r| matches!(r.level, DiagnosticLevel::Error) && r.message.contains("category")
            )
        );
    }

    #[test]
    fn test_check_entities_multiple_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let entities = dir.path().join("src/entities");
        std::fs::create_dir_all(entities.join("product")).unwrap();
        std::fs::create_dir_all(entities.join("category")).unwrap();
        std::fs::write(entities.join("mod.rs"), "").unwrap();

        let results = check_entities(dir.path());
        let warns: Vec<_> = results
            .iter()
            .filter(|r| matches!(r.level, DiagnosticLevel::Warn))
            .collect();
        assert_eq!(warns.len(), 2);
    }

    // ================================================================
    // check_websocket additional test: no main.rs
    // ================================================================

    #[test]
    fn test_check_websocket_enabled_but_no_main_rs() {
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

        let results = check_websocket(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("main.rs not found"));
    }

    // ================================================================
    // check_events tests
    // ================================================================

    #[test]
    fn test_check_events_no_config_no_event_bus() {
        let dir = tempfile::tempdir().unwrap();
        // No events.yaml, no main.rs → should return empty
        let results = check_events(dir.path());
        assert!(results.is_empty());
    }

    #[test]
    fn test_check_events_no_config_with_event_bus() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() {\n    server.with_default_event_bus();\n}\n",
        )
        .unwrap();

        let results = check_events(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Warn));
        assert!(results[0].message.contains("event bus"));
        assert!(results[0].message.contains("events.yaml not found"));
    }

    #[test]
    fn test_check_events_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/events.yaml"),
            "this is: [not: valid: yaml: {{{",
        )
        .unwrap();

        let results = check_events(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Error));
        assert!(results[0].message.contains("invalid YAML"));
    }

    #[test]
    fn test_check_events_empty_sinks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/events.yaml"),
            "event_sinks: []\nevent_flows: []\n",
        )
        .unwrap();

        let results = check_events(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Warn)
                    && r.message.contains("No event sinks"))
        );
    }

    #[test]
    fn test_check_events_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/events.yaml"),
            r#"event_sinks:
  - name: in-app
    type: in_app
event_flows:
  - name: notify
    trigger: "entity.created.*"
    steps:
      - type: deliver
        sink: in-app
"#,
        )
        .unwrap();

        let results = check_events(dir.path());
        assert_eq!(results.len(), 1);
        assert!(matches!(results[0].level, DiagnosticLevel::Pass));
        assert!(results[0].message.contains("1 sink(s)"));
        assert!(results[0].message.contains("1 flow(s)"));
        assert!(results[0].message.contains("all references valid"));
    }

    #[test]
    fn test_check_events_unknown_sink_reference() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("config/events.yaml"),
            r#"event_sinks:
  - name: in-app
    type: in_app
event_flows:
  - name: bad-flow
    trigger: "entity.created.*"
    steps:
      - type: deliver
        sink: nonexistent-sink
"#,
        )
        .unwrap();

        let results = check_events(dir.path());
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Warn)
                    && r.message.contains("unknown sink")
                    && r.message.contains("nonexistent-sink"))
        );
    }

    #[test]
    fn test_check_events_multiple_flows_mixed() {
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
  - name: good-flow
    trigger: "entity.created.*"
    steps:
      - type: deliver
        sink: in-app
  - name: bad-flow
    trigger: "entity.updated.*"
    steps:
      - type: deliver
        sink: missing-sink
"#,
        )
        .unwrap();

        let results = check_events(dir.path());
        // Should warn about missing-sink, not about the good flow
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Warn)
                    && r.message.contains("bad-flow")
                    && r.message.contains("missing-sink"))
        );
    }

    // ================================================================
    // Full project with websocket and grpc features
    // ================================================================

    #[test]
    fn test_run_checks_project_with_features() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();

        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "feature-project"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", version = "0.0.8", features = ["websocket", "grpc"] }
"#,
        )
        .unwrap();

        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    // Uses WebSocketExposure and GrpcExposure\n}\n",
        )
        .unwrap();

        std::fs::write(root.join("src/module.rs"), "fn entity_types() {}").unwrap();
        std::fs::write(root.join("src/stores.rs"), "pub struct Stores {}").unwrap();
        std::fs::write(
            root.join("config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        let results = run_checks(root);

        // Should have pass for WebSocket and gRPC
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Pass) && r.category == "WebSocket")
        );
        assert!(
            results
                .iter()
                .any(|r| matches!(r.level, DiagnosticLevel::Pass) && r.category == "gRPC")
        );
    }
}
