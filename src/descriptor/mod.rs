//! The project's module descriptor, as this-cli reads and writes it.
//!
//! Three files under `descriptor/` (all optional: a project without
//! `descriptor/module.yaml` behaves exactly as before):
//!
//! | file | owner | role |
//! |---|---|---|
//! | `module.yaml` | the developer (+ `this add …`) | identity, requires, links, errors, i18n, permissions |
//! | `descriptor.lock` | `this descriptor lock` | field -> proto tag, never moved nor reused |
//! | `module.descriptor.json` | `this descriptor export` | MATERIALIZED descriptor: module.yaml composed with the entity macros (read back from code) + locked tags |
//!
//! The JSON is derived, never edited: `this descriptor check` reports it stale.

pub mod code;
pub mod from_fields;
pub mod io;
pub mod lock;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use this_descriptor::{CheckProfile, CustomRegistry, EntityDesc, ModuleDescriptor};

use crate::utils::file_writer::FileWriter;
use lock::DescriptorLock;

pub const DIR: &str = "descriptor";
pub const MODULE_YAML: &str = "descriptor/module.yaml";
pub const LOCK: &str = "descriptor/descriptor.lock";
pub const MATERIALIZED: &str = "descriptor/module.descriptor.json";

/// Descriptor mode: `descriptor/module.yaml` exists.
pub fn enabled(root: &Path) -> bool {
    root.join(MODULE_YAML).is_file()
}

pub fn read_source(root: &Path) -> Result<Option<(String, ModuleDescriptor)>> {
    let path = root.join(MODULE_YAML);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let d = io::parse_source(&text)?;
    Ok(Some((text, d)))
}

pub fn read_lock(root: &Path) -> Result<DescriptorLock> {
    let path = root.join(LOCK);
    if !path.is_file() {
        return Ok(DescriptorLock::default());
    }
    DescriptorLock::parse(&std::fs::read_to_string(&path)?)
}

/// `src/entities/<name>/model.rs` of every entity.
pub fn model_paths(root: &Path) -> Vec<(String, PathBuf)> {
    let dir = root.join("src/entities");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out: Vec<(String, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("model.rs").is_file())
        .map(|e| {
            (
                e.file_name().to_string_lossy().to_string(),
                e.path().join("model.rs"),
            )
        })
        .collect();
    out.sort();
    out
}

/// Entity descriptors read back from the code. `overrides` replaces the content of a
/// model not written yet (dry run, or the entity being generated): `(dir name, content)`.
pub fn code_entities(root: &Path, overrides: &[(String, String)]) -> Result<Vec<EntityDesc>> {
    let mut out = Vec::new();
    for (dir, path) in model_paths(root) {
        if overrides.iter().any(|(n, _)| *n == dir) {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        if let Some(e) = code::entity_from_model(&content)
            .with_context(|| format!("reading {}", path.display()))?
        {
            out.push(e);
        }
    }
    for (dir, content) in overrides {
        if let Some(e) = code::entity_from_model(content)
            .with_context(|| format!("reading src/entities/{dir}/model.rs"))?
        {
            out.push(e);
        }
    }
    Ok(out)
}

/// Result of composing module.yaml with the code.
pub struct Materialized {
    /// Composed, normalized, tags of the lock applied.
    pub descriptor: ModuleDescriptor,
    /// Lock brought up to date.
    pub lock: DescriptorLock,
    pub lock_changes: Vec<String>,
}

/// `source.compose(code)`, then the lock: refused if an explicit tag contradicts it,
/// otherwise updated (new fields get tags, removed fields are reserved) and applied.
pub fn materialize(
    root: &Path,
    source: ModuleDescriptor,
    overrides: &[(String, String)],
) -> Result<Materialized> {
    let described = code_entities(root, overrides)?;
    let refs: Vec<&EntityDesc> = described.iter().collect();
    let mut composed = source.compose(&refs).map_err(io::contract)?;
    let mut lock = read_lock(root)?;
    let issues = lock.check(&composed);
    if !issues.is_empty() {
        anyhow::bail!(
            "descriptor.lock: {}",
            issues
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    let lock_changes = lock.update(&composed);
    lock.apply(&mut composed);
    Ok(Materialized {
        descriptor: composed,
        lock,
        lock_changes,
    })
}

/// Write `path` (create or update) only when its content changes.
pub fn write_if_changed(root: &Path, rel: &str, content: &str, w: &dyn FileWriter) -> Result<bool> {
    let path = root.join(rel);
    match std::fs::read_to_string(&path) {
        Ok(old) if old == content => Ok(false),
        Ok(old) => w.update_file(&path, &old, content).map(|_| true),
        Err(_) => {
            if let Some(parent) = path.parent()
                && !parent.exists()
            {
                w.create_dir_all(parent)?;
            }
            w.write_file(&path, content).map(|_| true)
        }
    }
}

pub fn materialized_json(d: &ModuleDescriptor) -> String {
    let mut s = d.to_json_pretty();
    s.push('\n');
    s
}

/// Write `descriptor.lock` and `module.descriptor.json`.
pub fn write_outputs(root: &Path, m: &Materialized, w: &dyn FileWriter) -> Result<()> {
    write_if_changed(root, LOCK, &m.lock.to_yaml(), w)?;
    write_if_changed(root, MATERIALIZED, &materialized_json(&m.descriptor), w)?;
    Ok(())
}

/// Re-materialize after an edit (`this add entity/link/module`) and write the outputs.
pub fn refresh(
    root: &Path,
    source_text: &str,
    overrides: &[(String, String)],
    w: &dyn FileWriter,
) -> Result<Materialized> {
    let source = io::parse_source(source_text)?;
    let m = materialize(root, source, overrides)?;
    write_outputs(root, &m, w)?;
    Ok(m)
}

/// Outcome of `this descriptor check`.
#[derive(Debug, Default, serde::Serialize)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Every verification on one project. `Ok(None)` without descriptor/module.yaml.
pub fn check_project(root: &Path) -> Result<Option<Report>> {
    let mut r = Report::default();
    let text = match std::fs::read_to_string(root.join(MODULE_YAML)) {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    let source = match io::parse_source(&text) {
        Ok(s) => s,
        Err(e) => {
            r.errors.push(format!("{e:#}"));
            return Ok(Some(r));
        }
    };
    let described = match code_entities(root, &[]) {
        Ok(d) => d,
        Err(e) => {
            r.errors.push(format!("{e:#}"));
            return Ok(Some(r));
        }
    };
    let refs: Vec<&EntityDesc> = described.iter().collect();
    let mut composed = match source.compose(&refs) {
        Ok(c) => c,
        Err(e) => {
            r.errors.push(format!("{:#}", io::contract(e)));
            return Ok(Some(r));
        }
    };
    let lock = match read_lock(root) {
        Ok(l) => l,
        Err(e) => {
            r.errors.push(format!("{e:#}"));
            return Ok(Some(r));
        }
    };
    r.errors
        .extend(lock.check(&composed).iter().map(|i| i.to_string()));
    r.warnings.extend(
        lock.pending(&composed)
            .into_iter()
            .map(|p| format!("{p} (run `this descriptor lock`)")),
    );
    lock.apply(&mut composed);
    if let Err(issues) = composed.check(&CustomRegistry::default(), CheckProfile::Compiled) {
        r.errors.extend(issues.iter().map(|i| i.to_string()));
    }
    match std::fs::read_to_string(root.join(MATERIALIZED)) {
        Ok(old) if old != materialized_json(&composed) => r.warnings.push(format!(
            "{MATERIALIZED} is stale (run `this descriptor export`)"
        )),
        Ok(_) => {}
        Err(_) => r.warnings.push(format!(
            "{MATERIALIZED} is missing (run `this descriptor export`)"
        )),
    }
    Ok(Some(r))
}

/// Materialized descriptor for read-only consumers (`info --json`, introspection):
/// the lock is applied as it is, never updated.
pub fn current(root: &Path) -> Result<Option<ModuleDescriptor>> {
    let Some((_, source)) = read_source(root)? else {
        return Ok(None);
    };
    let described = code_entities(root, &[])?;
    let refs: Vec<&EntityDesc> = described.iter().collect();
    let mut composed = source.compose(&refs).map_err(io::contract)?;
    read_lock(root)?.apply(&mut composed);
    Ok(Some(composed))
}

/// Marker of the entity list composed by the generated `Module::descriptor()`.
pub const DESCRIBED_MARKER: &str = "[this:described_entities]";

/// Same body as `templates/project/module.rs.tera` renders with `descriptor = true`.
pub const MODULE_DESCRIPTOR_FN: &str = r#"
    /// Module descriptor: descriptor/module.yaml (identity, links, permissions, errors...)
    /// composed with the entity macros (fields, types, rules). See `this descriptor check`.
    fn descriptor(&self) -> Option<this::descriptor::ModuleDescriptor> {
        let yaml = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/descriptor/module.yaml"));
        let described: Vec<this::descriptor::EntityDesc> = vec![
            // [this:described_entities]
        ];
        let refs: Vec<&this::descriptor::EntityDesc> = described.iter().collect();
        let module = this::descriptor::ModuleDescriptor::from_yaml(yaml)
            .and_then(|m| m.compose(&refs))
            .unwrap_or_else(|e| panic!("descriptor/module.yaml: {e} {:?}", e.params));
        Some(module)
    }
"#;

/// `src/module.rs` with `fn descriptor()` (inserted before the closing brace of the
/// `impl Module` block, the last `}` of the file) if it has none.
pub fn with_module_fn(content: &str) -> Result<String> {
    if content.contains(DESCRIBED_MARKER) || content.contains("fn descriptor(&self)") {
        return Ok(content.to_string());
    }
    let at = content
        .rfind("\n}")
        .context("src/module.rs: closing brace of `impl Module` not found")?;
    let mut out = content.to_string();
    out.insert_str(at, MODULE_DESCRIPTOR_FN.trim_end_matches('\n'));
    Ok(out)
}

/// Register an entity descriptor expression (idempotent).
pub fn with_described(content: &str, expr: &str) -> Result<String> {
    if !content.contains(DESCRIBED_MARKER) {
        anyhow::bail!(
            "src/module.rs has a custom `fn descriptor()` without the {DESCRIBED_MARKER} marker: \
             add `{expr},` to it by hand"
        );
    }
    if crate::utils::markers::has_line_after_marker(content, DESCRIBED_MARKER, expr) {
        return Ok(content.to_string());
    }
    crate::utils::markers::insert_after_marker(content, DESCRIBED_MARKER, &format!("{expr},"))
}

/// Expression giving the descriptor of an entity in `Module::descriptor()`: `described()`
/// when the model has one (entities generated in descriptor mode), else the macro's.
pub fn described_expr(entity: &str, pascal: &str, model: &str) -> String {
    if model.contains("pub fn described()") {
        format!("crate::entities::{entity}::model::described()")
    } else {
        format!("crate::entities::{entity}::{pascal}::descriptor().clone()")
    }
}
