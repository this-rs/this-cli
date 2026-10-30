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
    DescriptorLock::parse(&std::fs::read_to_string(&path)?).map_err(io::contract)
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
    let (errors, warnings) = registration_issues(root)?;
    r.errors.extend(errors);
    r.warnings.extend(warnings);
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

/// Expressions listed after [`DESCRIBED_MARKER`] in `src/module.rs`, up to the closing `]`
/// of the `vec![...]`; `None` without the marker.
fn described_list(module_rs: &str) -> Option<&str> {
    let at = module_rs.find(DESCRIBED_MARKER)?;
    let rest = &module_rs[at + DESCRIBED_MARKER.len()..];
    Some(&rest[..rest.find("];").unwrap_or(rest.len())])
}

/// What the server composes (`Module::descriptor()` in src/module.rs) must be what the
/// CLI materializes from the models. `ModuleDescriptor::compose` keeps a module.yaml entity
/// that no `described()` completes as a stub WITHOUT fields: an entity missing from the
/// list reaches the runtime with no field and no rule, and one registered through the
/// macro's `descriptor()` while its model has `described()` loses its DESCRIBED_EXTRA rules
/// (email, min_value, filters...). Both are errors; a hand-written `fn descriptor()` without
/// the marker cannot be verified (warning).
pub fn registration_issues(root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let (mut errors, mut warnings) = (Vec::new(), Vec::new());
    let Ok(module_rs) = std::fs::read_to_string(root.join("src/module.rs")) else {
        warnings.push("src/module.rs not found: Module::descriptor() not verified".into());
        return Ok((errors, warnings));
    };
    if !module_rs.contains("fn descriptor(&self)") {
        errors.push(format!(
            "src/module.rs has no fn descriptor(): the server ignores {MODULE_YAML} \
             (add the body of `this descriptor init`, see docs/module-descriptor.md)"
        ));
        return Ok((errors, warnings));
    }
    let Some(list) = described_list(&module_rs) else {
        warnings.push(format!(
            "src/module.rs: fn descriptor() has no {DESCRIBED_MARKER} marker: the entities it \
             composes are not verified"
        ));
        return Ok((errors, warnings));
    };
    for (dir, path) in model_paths(root) {
        let model = std::fs::read_to_string(&path)?;
        if code::entity_from_model(&model)
            .with_context(|| format!("reading {}", path.display()))?
            .is_none()
        {
            continue;
        }
        let pascal = crate::utils::naming::to_pascal_case(&dir);
        let expected = described_expr(&dir, &pascal, &model);
        if list.contains(&expected) {
            continue;
        }
        if list.contains(&format!("crate::entities::{dir}::")) {
            errors.push(format!(
                "src/module.rs: entity '{dir}' is composed without its described() \
                 (expected `{expected}`): its DESCRIBED_EXTRA rules never reach the server"
            ));
        } else {
            errors.push(format!(
                "src/module.rs: entity '{dir}' missing from Module::descriptor() \
                 (add `{expected},` after {DESCRIBED_MARKER}): the server would see it \
                 without fields nor rules"
            ));
        }
    }
    Ok((errors, warnings))
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

/// Body of `Module::descriptor()` inserted by `this descriptor init` / `this add entity`.
/// The ONLY copy (module.rs.tera no longer carries a second one: `this init` does not
/// enable descriptor mode).
pub const MODULE_DESCRIPTOR_FN: &str = r#"
    /// Module descriptor: descriptor/module.yaml (identity, links, permissions, errors...)
    /// composed with the entity macros (fields, types, rules) and descriptor.lock (proto tags).
    /// See `this descriptor check`.
    fn descriptor(&self) -> Option<this::descriptor::ModuleDescriptor> {
        let yaml = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/descriptor/module.yaml"));
        // descriptor.lock: the stable proto tags (`/grpc/proto`, `this descriptor export`).
        let lock = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/descriptor/descriptor.lock"));
        let described: Vec<this::descriptor::EntityDesc> = vec![
            // [this:described_entities]
        ];
        let refs: Vec<&this::descriptor::EntityDesc> = described.iter().collect();
        let module = this::descriptor::ModuleDescriptor::from_yaml(yaml)
            .and_then(|m| m.compose_locked(&refs, lock))
            .unwrap_or_else(|e| panic!("descriptor/module.yaml + descriptor.lock: {e} {:?}", e.params));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::file_writer::RealWriter;
    use std::fs;

    const MODEL: &str = r#"impl_data_entity!(Order, "order", ["name"], { sku: String });"#;

    /// Project with `descriptor/module.yaml` declaring `order` and its model.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("descriptor")).unwrap();
        fs::create_dir_all(root.join("src/entities/order")).unwrap();
        fs::write(
            root.join(MODULE_YAML),
            io::initial_source("shop", "1.0.0", &["order".to_string()]),
        )
        .unwrap();
        fs::write(root.join("src/entities/order/model.rs"), MODEL).unwrap();
        dir
    }

    fn err_chain<T>(r: Result<T>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => format!("{e:#}"),
        }
    }

    #[test]
    fn enabled_and_read_source_follow_module_yaml() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!enabled(dir.path()));
        assert!(read_source(dir.path()).unwrap().is_none());
        assert!(check_project(dir.path()).unwrap().is_none());
        assert!(current(dir.path()).unwrap().is_none());
        let p = project();
        assert!(enabled(p.path()));
        let (text, d) = read_source(p.path()).unwrap().unwrap();
        assert!(text.contains("name: shop"));
        assert_eq!(d.entities[0].name, "order");
    }

    #[test]
    fn code_entities_reads_models_and_overrides_replace_them() {
        let p = project();
        let from_disk = code_entities(p.path(), &[]).unwrap();
        assert_eq!(from_disk.len(), 1);
        assert!(from_disk[0].fields.iter().any(|f| f.name == "sku"));
        // The override replaces the file content of the same directory.
        let over = vec![(
            "order".to_string(),
            r#"impl_data_entity!(Order, "order", [], { title: String });"#.to_string(),
        )];
        let got = code_entities(p.path(), &over).unwrap();
        assert_eq!(got.len(), 1);
        assert!(got[0].fields.iter().any(|f| f.name == "title"));
        assert!(!got[0].fields.iter().any(|f| f.name == "sku"));
        // A model without the macro is ignored.
        fs::create_dir_all(p.path().join("src/entities/plain")).unwrap();
        fs::write(
            p.path().join("src/entities/plain/model.rs"),
            "pub struct P;",
        )
        .unwrap();
        assert_eq!(code_entities(p.path(), &[]).unwrap().len(), 1);
        assert_eq!(
            model_paths(p.path())
                .into_iter()
                .map(|(n, _)| n)
                .collect::<Vec<_>>(),
            vec!["order", "plain"]
        );
    }

    #[test]
    fn materialize_refuses_a_corrupt_lock_and_names_the_issue() {
        let p = project();
        fs::write(
            p.path().join(LOCK),
            "format: 1\nentities:\n  order:\n    fields:\n      sku: 16\n      title: 16\n",
        )
        .unwrap();
        let (_, source) = read_source(p.path()).unwrap().unwrap();
        let err = match materialize(p.path(), source, &[]) {
            Ok(_) => panic!("a corrupt lock must be refused"),
            Err(e) => e.to_string(),
        };
        assert!(err.starts_with("descriptor.lock: "), "{err}");
        assert!(err.contains("tag 16 held by `sku` and `title`"), "{err}");
    }

    #[test]
    fn materialize_assigns_tags_and_refresh_writes_outputs_once() {
        let p = project();
        let (text, _) = read_source(p.path()).unwrap().unwrap();
        let m = refresh(p.path(), &text, &[], &RealWriter).unwrap();
        let sku = m.descriptor.entities[0]
            .fields
            .iter()
            .find(|f| f.name == "sku")
            .unwrap();
        assert_eq!(sku.proto_tag, Some(16));
        assert!(!m.lock_changes.is_empty());
        let lock = fs::read_to_string(p.path().join(LOCK)).unwrap();
        assert!(lock.contains("sku: 16"), "{lock}");
        let json = fs::read_to_string(p.path().join(MATERIALIZED)).unwrap();
        assert_eq!(json, materialized_json(&m.descriptor));
        assert!(json.ends_with("}\n"));
        // Nothing changed: write_outputs rewrites nothing.
        assert!(!write_if_changed(p.path(), LOCK, &m.lock.to_yaml(), &RealWriter).unwrap());
        assert_eq!(read_lock(p.path()).unwrap(), m.lock);
        assert_eq!(current(p.path()).unwrap().unwrap(), m.descriptor);
    }

    #[test]
    fn write_if_changed_creates_parents_then_updates() {
        let dir = tempfile::tempdir().unwrap();
        let w = RealWriter;
        assert!(write_if_changed(dir.path(), "a/b/c.txt", "one", &w).unwrap());
        assert!(!write_if_changed(dir.path(), "a/b/c.txt", "one", &w).unwrap());
        assert!(write_if_changed(dir.path(), "a/b/c.txt", "two", &w).unwrap());
        assert_eq!(
            fs::read_to_string(dir.path().join("a/b/c.txt")).unwrap(),
            "two"
        );
    }

    #[test]
    fn check_project_reports_unreadable_code_and_lock() {
        let p = project();
        // A model rule outside the macro grammar.
        fs::write(
            p.path().join("src/entities/order/model.rs"),
            r#"impl_data_entity_validated!(Order, "order", [], { sku: String },
               validate: { create: { sku: [email] } });"#,
        )
        .unwrap();
        let r = check_project(p.path()).unwrap().unwrap();
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert!(r.errors[0].contains("reading "), "{:?}", r.errors);
        assert!(r.errors[0].contains("model.rs"), "{:?}", r.errors);
        assert!(r.errors[0].contains("validator `email` is not in"));

        let p = project();
        fs::write(p.path().join(LOCK), "format: [not, a, number]\n").unwrap();
        let r = check_project(p.path()).unwrap().unwrap();
        assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
        assert!(r.errors[0].contains("descriptor.lock is not valid YAML"));
    }

    #[test]
    fn check_project_warns_on_missing_then_stale_materialized_json() {
        let p = project();
        let r = check_project(p.path()).unwrap().unwrap();
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(
            r.warnings
                .iter()
                .any(|w| w.starts_with("src/module.rs not found")),
            "{:?}",
            r.warnings
        );
        assert!(
            r.warnings
                .iter()
                .any(|w| w == &format!("{MATERIALIZED} is missing (run `this descriptor export`)")),
            "{:?}",
            r.warnings
        );
        let (text, _) = read_source(p.path()).unwrap().unwrap();
        refresh(p.path(), &text, &[], &RealWriter).unwrap();
        let r = check_project(p.path()).unwrap().unwrap();
        assert!(
            !r.warnings.iter().any(|w| w.contains(MATERIALIZED)),
            "{:?}",
            r.warnings
        );
        fs::write(p.path().join(MATERIALIZED), "{}\n").unwrap();
        let r = check_project(p.path()).unwrap().unwrap();
        assert!(
            r.warnings
                .iter()
                .any(|w| w == &format!("{MATERIALIZED} is stale (run `this descriptor export`)")),
            "{:?}",
            r.warnings
        );
    }

    #[test]
    fn registration_issues_covers_each_module_rs_state() {
        let p = project();
        // A model without the macro is not checked.
        fs::create_dir_all(p.path().join("src/entities/plain")).unwrap();
        fs::write(
            p.path().join("src/entities/plain/model.rs"),
            "pub struct P;",
        )
        .unwrap();
        let module_rs = p.path().join("src/module.rs");

        let (e, w) = registration_issues(p.path()).unwrap();
        assert!(e.is_empty());
        assert_eq!(
            w,
            vec!["src/module.rs not found: Module::descriptor() not verified"]
        );

        fs::write(&module_rs, "impl Module for M {}").unwrap();
        let (e, w) = registration_issues(p.path()).unwrap();
        assert!(w.is_empty());
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("has no fn descriptor()"), "{e:?}");

        fs::write(&module_rs, "fn descriptor(&self) { custom() }").unwrap();
        let (e, w) = registration_issues(p.path()).unwrap();
        assert!(e.is_empty());
        assert_eq!(w.len(), 1);
        assert!(
            w[0].contains("has no [this:described_entities] marker"),
            "{w:?}"
        );

        // Marker present, entity missing from the list.
        let with_marker = |list: &str| {
            format!("fn descriptor(&self) {{\n vec![\n // {DESCRIBED_MARKER}\n {list}\n ];\n}}")
        };
        fs::write(&module_rs, with_marker("")).unwrap();
        let (e, _) = registration_issues(p.path()).unwrap();
        assert_eq!(e.len(), 1);
        assert!(
            e[0].contains("entity 'order' missing from Module::descriptor()"),
            "{e:?}"
        );
        assert!(e[0].contains("crate::entities::order::Order::descriptor().clone(),"));

        // Registered through the macro while the model has described().
        fs::write(
            p.path().join("src/entities/order/model.rs"),
            format!("{MODEL}\npub fn described() -> EntityDesc {{ todo!() }}"),
        )
        .unwrap();
        fs::write(
            &module_rs,
            with_marker("crate::entities::order::Order::descriptor().clone(),"),
        )
        .unwrap();
        let (e, _) = registration_issues(p.path()).unwrap();
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("composed without its described()"), "{e:?}");
        assert!(e[0].contains("crate::entities::order::model::described()"));

        // Registered correctly.
        fs::write(
            &module_rs,
            with_marker("crate::entities::order::model::described(),"),
        )
        .unwrap();
        let (e, w) = registration_issues(p.path()).unwrap();
        assert!(e.is_empty() && w.is_empty(), "{e:?} {w:?}");
    }

    #[test]
    fn described_expr_picks_described_or_macro_descriptor() {
        assert_eq!(
            described_expr("order", "Order", "pub fn described() -> X {}"),
            "crate::entities::order::model::described()"
        );
        assert_eq!(
            described_expr("order", "Order", MODEL),
            "crate::entities::order::Order::descriptor().clone()"
        );
    }

    #[test]
    fn with_module_fn_inserts_once_before_the_closing_brace() {
        let src = "impl Module for M {\n    fn name(&self) {}\n}\n";
        let out = with_module_fn(src).unwrap();
        assert!(out.contains(DESCRIBED_MARKER));
        assert!(out.contains("fn descriptor(&self)"));
        assert!(out.find("fn name").unwrap() < out.find("fn descriptor").unwrap());
        assert!(out.trim_end().ends_with('}'));
        assert_eq!(with_module_fn(&out).unwrap(), out, "idempotent");
        assert_eq!(
            with_module_fn("fn descriptor(&self) {}").unwrap(),
            "fn descriptor(&self) {}"
        );
        let e = err_chain(with_module_fn("no braces"));
        assert!(
            e.contains("closing brace of `impl Module` not found"),
            "{e}"
        );
    }

    #[test]
    fn with_described_inserts_idempotently_and_refuses_custom_descriptor() {
        let src = format!("  vec![\n    // {DESCRIBED_MARKER}\n  ]");
        let expr = "crate::entities::order::model::described()";
        let once = with_described(&src, expr).unwrap();
        assert!(once.contains(&format!("    {expr},")), "{once}");
        assert_eq!(with_described(&once, expr).unwrap(), once, "idempotent");
        let e = err_chain(with_described("fn descriptor(&self) {}", expr));
        assert!(
            e.contains("custom `fn descriptor()` without the [this:described_entities] marker"),
            "{e}"
        );
        assert!(e.contains(&format!("add `{expr},` to it by hand")), "{e}");
    }
}
