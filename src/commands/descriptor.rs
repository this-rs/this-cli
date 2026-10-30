//! `this descriptor init|check|export|lock` and `this add module <manifest>`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use colored::Colorize;
use this_descriptor::engine::validate::WriteOp;
use this_descriptor::{CheckProfile, CustomRegistry, DescriptorSet, ModuleDescriptor, ModuleReq};

use super::{AddModuleArgs, DescriptorCommands, DescriptorExportArgs};
use crate::descriptor::{self, io};
use crate::utils::file_writer::FileWriter;
use crate::utils::{naming, output, project};

pub fn run(cmd: DescriptorCommands, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    run_in(cmd, writer, &cwd)
}

pub(crate) fn run_in(cmd: DescriptorCommands, writer: &dyn FileWriter, cwd: &Path) -> Result<()> {
    let root = project::detect_project_root_from(cwd)?;
    match cmd {
        DescriptorCommands::Init(a) => init(&root, a.module_version.as_deref(), writer),
        DescriptorCommands::Check(a) => {
            let report = check(&root)?;
            print_report(&report, a.json);
            if report.failed(a.strict) {
                bail!(
                    "descriptor check failed: {} error(s), {} warning(s)",
                    report.errors.len(),
                    report.warnings.len()
                );
            }
            Ok(())
        }
        DescriptorCommands::Export(a) => export(&root, &a, writer),
        DescriptorCommands::Lock => lock(&root, writer),
    }
}

/// Module name derived from `src/module.rs` (`fn name(&self) -> &str { "..." }`), else
/// from Cargo.toml: the runtime compares `to_snake_identifier(Module::name())`.
fn module_name(root: &Path) -> String {
    let from_module = std::fs::read_to_string(root.join("src/module.rs"))
        .ok()
        .and_then(|c| {
            regex::Regex::new(r#"fn name\(&self\)\s*->\s*&str\s*\{\s*"([^"]+)""#)
                .unwrap()
                .captures(&c)
                .map(|m| m[1].to_string())
        });
    let raw = from_module.unwrap_or_else(|| {
        std::fs::read_to_string(root.join("Cargo.toml"))
            .ok()
            .and_then(|c| c.parse::<toml_edit::DocumentMut>().ok())
            .and_then(|d| d["package"]["name"].as_str().map(str::to_string))
            .unwrap_or_else(|| "app".into())
    });
    this_descriptor::naming::to_snake_identifier(&raw)
}

/// `Module::version()` (default of the trait: "1.0.0").
fn module_version(root: &Path) -> String {
    std::fs::read_to_string(root.join("src/module.rs"))
        .ok()
        .and_then(|c| {
            regex::Regex::new(r#"fn version\(&self\)\s*->\s*&str\s*\{\s*"([^"]+)""#)
                .unwrap()
                .captures(&c)
                .map(|m| m[1].to_string())
        })
        .unwrap_or_else(|| "1.0.0".into())
}

fn init(root: &Path, version: Option<&str>, w: &dyn FileWriter) -> Result<()> {
    let yaml_path = root.join(descriptor::MODULE_YAML);
    if yaml_path.exists() {
        bail!("{} already exists", descriptor::MODULE_YAML);
    }
    let entities: Vec<String> = descriptor::code_entities(root, &[])?
        .into_iter()
        .map(|e| e.name)
        .collect();
    let version = version
        .map(str::to_string)
        .unwrap_or_else(|| module_version(root));
    let text = io::initial_source(&module_name(root), &version, &entities);
    io::parse_source(&text)?;
    w.create_dir_all(&root.join(descriptor::DIR))?;
    w.write_file(&yaml_path, &text)?;

    // Module::descriptor() in src/module.rs, listing the existing entities.
    let module_path = root.join("src/module.rs");
    if module_path.exists() {
        let content = std::fs::read_to_string(&module_path)?;
        let mut updated = descriptor::with_module_fn(&content)?;
        for (dir, model) in descriptor::model_paths(root) {
            let model = std::fs::read_to_string(model)?;
            let pascal = naming::to_pascal_case(&dir);
            updated = descriptor::with_described(
                &updated,
                &descriptor::described_expr(&dir, &pascal, &model),
            )?;
        }
        if updated != content {
            w.update_file(&module_path, &content, &updated)?;
        }
    } else {
        output::print_warn("src/module.rs not found: add `fn descriptor()` to your Module by hand");
    }

    let m = descriptor::refresh(root, &text, &[], w)?;
    output::print_success(&format!(
        "Descriptor mode enabled: {} ({} entit{})",
        descriptor::MODULE_YAML,
        m.descriptor.entities.len(),
        if m.descriptor.entities.len() == 1 {
            "y"
        } else {
            "ies"
        }
    ));
    output::print_next_steps(&[
        "Declare permissions, links, errors, i18n keys in descriptor/module.yaml",
        "this descriptor check   # verify (exit code 1 on error)",
        "this descriptor export  # refresh descriptor/module.descriptor.json",
    ]);
    Ok(())
}

/// Report of `this descriptor check`.
#[derive(Debug, Default, serde::Serialize)]
pub struct CheckReport {
    pub descriptor: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl CheckReport {
    pub fn failed(&self, strict: bool) -> bool {
        !self.errors.is_empty() || (strict && !self.warnings.is_empty())
    }
}

pub fn check(root: &Path) -> Result<CheckReport> {
    Ok(match descriptor::check_project(root)? {
        None => CheckReport {
            descriptor: false,
            errors: vec![],
            warnings: vec![format!(
                "no {}: nothing to check (run `this descriptor init`)",
                descriptor::MODULE_YAML
            )],
        },
        Some(r) => CheckReport {
            descriptor: true,
            errors: r.errors,
            warnings: r.warnings,
        },
    })
}

fn print_report(r: &CheckReport, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(r).unwrap_or_default());
        return;
    }
    for e in &r.errors {
        println!("  {} {}", "✗".red().bold(), e);
    }
    for w in &r.warnings {
        println!("  {} {}", "⚠".yellow().bold(), w);
    }
    if r.errors.is_empty() && r.warnings.is_empty() {
        output::print_success("Descriptor OK");
    }
}

fn export(root: &Path, a: &DescriptorExportArgs, w: &dyn FileWriter) -> Result<()> {
    let Some((text, _)) = descriptor::read_source(root)? else {
        bail!(
            "no {}: run `this descriptor init` first",
            descriptor::MODULE_YAML
        );
    };
    let source = io::parse_source(&text)?;
    let m = descriptor::materialize(root, source, &[])?;
    let d = &m.descriptor;
    let rendered = match a.format.as_str() {
        "json" => descriptor::materialized_json(d),
        "yaml" => d.to_yaml(),
        "graphql" | "sdl" => derive_set(d, |s| {
            this_descriptor::derive::graphql_sdl(s).map_err(io::contract)
        })?,
        "proto" => derive_set(d, |s| {
            this_descriptor::derive::proto_file(s, &d.name).map_err(io::contract)
        })?,
        "json-schema" => {
            let mut out = serde_json::Map::new();
            for e in &d.entities {
                for (op, name) in [(WriteOp::Create, "create"), (WriteOp::Update, "update")] {
                    let schema =
                        this_descriptor::derive::json_schema(e, op).map_err(io::contract)?;
                    out.insert(format!("{}.{name}", e.name), schema);
                }
            }
            serde_json::to_string_pretty(&out)? + "\n"
        }
        other => bail!("unknown format `{other}` (json, yaml, graphql, proto, json-schema)"),
    };
    match (&a.output, a.stdout, a.format.as_str()) {
        (None, false, "json") => {
            descriptor::write_outputs(root, &m, w)?;
            output::print_success(&format!(
                "Wrote {} ({} entities, {} links)",
                descriptor::MATERIALIZED,
                d.entities.len(),
                d.links.len()
            ));
        }
        (Some(path), _, _) => {
            let path = if path.is_absolute() {
                path.clone()
            } else {
                root.join(path)
            };
            match std::fs::read_to_string(&path) {
                Ok(old) => w.update_file(&path, &old, &rendered)?,
                Err(_) => {
                    if let Some(parent) = path.parent() {
                        w.create_dir_all(parent)?;
                    }
                    w.write_file(&path, &rendered)?
                }
            }
            output::print_success(&format!("Wrote {}", path.display()));
        }
        _ => print!("{rendered}"),
    }
    Ok(())
}

fn derive_set(
    d: &ModuleDescriptor,
    f: impl Fn(&DescriptorSet) -> Result<String>,
) -> Result<String> {
    let mut set = DescriptorSet::new();
    set.insert(d.clone()).map_err(io::contract)?;
    f(&set)
}

fn lock(root: &Path, w: &dyn FileWriter) -> Result<()> {
    let Some((text, _)) = descriptor::read_source(root)? else {
        bail!(
            "no {}: run `this descriptor init` first",
            descriptor::MODULE_YAML
        );
    };
    let m = descriptor::refresh(root, &text, &[], w)?;
    if m.lock_changes.is_empty() {
        output::print_info("descriptor.lock is up to date");
    } else {
        for c in &m.lock_changes {
            output::print_info(c);
        }
        output::print_success(&format!("Updated {}", descriptor::LOCK));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// this add module <manifest>
// ---------------------------------------------------------------------------

/// Read a module manifest: a descriptor in JSON (`module.descriptor.json`) or YAML.
pub fn read_manifest(path: &Path) -> Result<ModuleDescriptor> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let is_json =
        path.extension().is_some_and(|e| e == "json") || text.trim_start().starts_with('{');
    let d = if is_json {
        ModuleDescriptor::from_json(&text)
    } else {
        ModuleDescriptor::from_yaml(&text)
    }
    .map_err(io::contract)
    .with_context(|| format!("{} is not a module descriptor", path.display()))?;
    // The manifest describes a compiled module (its `module.descriptor.json`): the
    // Compiled profile. (The former `Stored`-then-`Compiled` fallback accepted exactly what
    // Compiled accepts while claiming the stricter profile.)
    d.check(&CustomRegistry::default(), CheckProfile::Compiled)
        .map_err(|issues| io::contract(this_descriptor::check::issues_to_error(&issues)))
        .with_context(|| format!("{} does not pass check()", path.display()))?;
    Ok(d)
}

pub fn add_module(args: AddModuleArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    add_module_in(args, writer, &cwd)
}

/// Declare a dependency on another module from its manifest: the manifest is checked,
/// copied under `descriptor/modules/<name>.json` (what the module exposes: entities,
/// links, errors, operations — its runtime interface), and `requires` gets
/// `{ name, version: ^<version>, optional }`.
pub(crate) fn add_module_in(
    args: AddModuleArgs,
    writer: &dyn FileWriter,
    cwd: &Path,
) -> Result<()> {
    let root = project::detect_project_root_from(cwd)?;
    let Some((text, own)) = descriptor::read_source(&root)? else {
        bail!(
            "no {}: run `this descriptor init` first",
            descriptor::MODULE_YAML
        );
    };
    let manifest_path: PathBuf = if args.manifest.is_absolute() {
        args.manifest.clone()
    } else {
        cwd.join(&args.manifest)
    };
    let dep = read_manifest(&manifest_path)?;
    if dep.name == own.name {
        bail!("a module cannot require itself (`{}`)", dep.name);
    }
    let version = match args.version_req {
        Some(v) => v,
        None => {
            let v = dep
                .semver()
                .with_context(|| format!("manifest version `{}` is not semver", dep.version))?;
            format!("^{v}")
        }
    };
    semver::VersionReq::parse(&version)
        .with_context(|| format!("`{version}` is not a semver requirement"))?;
    let req = ModuleReq {
        name: dep.name.clone(),
        version,
        optional: args.optional,
    };
    let new_text = io::add_requirement(&text, &req)?;
    writer.update_file(&root.join(descriptor::MODULE_YAML), &text, &new_text)?;
    let copy = format!("{}/modules/{}.json", descriptor::DIR, dep.name);
    descriptor::write_if_changed(&root, &copy, &descriptor::materialized_json(&dep), writer)?;
    descriptor::refresh(&root, &new_text, &[], writer)?;

    output::print_success(&format!(
        "Module `{}` {} required ({}{})",
        dep.name,
        dep.version,
        req.version,
        if req.optional { ", optional" } else { "" }
    ));
    output::print_info(&format!(
        "Interface: {} entit{}, {} link(s), {} error code(s) — copied to {copy}",
        dep.entities.len(),
        if dep.entities.len() == 1 { "y" } else { "ies" },
        dep.links.len(),
        dep.errors.len()
    ));
    Ok(())
}
