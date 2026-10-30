//! Read and edit `descriptor/module.yaml`.
//!
//! Edits go through `serde_yaml::Value`, never through a typed round-trip: keys this CLI
//! does not know (a newer descriptor format, a hand-written `custom:`) are kept, the way
//! the `#[serde(flatten)] extra` maps keep them in `links.yaml`. Every edited document is
//! re-parsed as a [`ModuleDescriptor`] before it is written: an edit never produces a file
//! the runtime would refuse to parse.

use anyhow::{Context, Result, bail};
use serde_yaml::{Mapping, Value};
use this_descriptor::{ContractError, LinkDesc, ModuleDescriptor, ModuleReq};

/// `ContractError` with its params (the `Display` shows only code and message).
pub fn contract(e: ContractError) -> anyhow::Error {
    if e.params.is_empty() {
        anyhow::anyhow!("{e}")
    } else {
        let params: Vec<String> = e
            .params
            .iter()
            .map(|(k, v)| match v {
                serde_json::Value::String(s) => format!("{k}: {s}"),
                v => format!("{k}: {v}"),
            })
            .collect();
        anyhow::anyhow!("{e} ({})", params.join("; "))
    }
}

pub fn parse_source(text: &str) -> Result<ModuleDescriptor> {
    ModuleDescriptor::from_yaml(text)
        .map_err(contract)
        .context("descriptor/module.yaml")
}

/// Initial `descriptor/module.yaml` of a project.
pub fn initial_source(name: &str, version: &str, entities: &[String]) -> String {
    let mut s = format!(
        "# Module descriptor (RFC module-descriptor). Owns: identity, requires, links, errors,\n\
         # i18n keys, and per entity: permissions, labels, capabilities, reserved tags.\n\
         # Fields, types and field rules belong to the entity macros (src/entities/*/model.rs):\n\
         # do NOT list `fields:` here (this.descriptor.declared_twice).\n\
         format: 1\nname: {name}\nversion: {version}\n"
    );
    if entities.is_empty() {
        s.push_str("entities: []\n");
    } else {
        s.push_str("entities:\n");
        for e in entities {
            s.push_str(&format!("  - name: {e}\n"));
        }
    }
    s
}

fn load(text: &str) -> Result<Mapping> {
    match serde_yaml::from_str::<Value>(text).context("descriptor/module.yaml is not YAML")? {
        Value::Mapping(m) => Ok(m),
        Value::Null => Ok(Mapping::new()),
        _ => bail!("descriptor/module.yaml must be a mapping"),
    }
}

fn seq<'a>(m: &'a mut Mapping, key: &str) -> Result<&'a mut Vec<Value>> {
    let k = Value::String(key.into());
    if !matches!(m.get(&k), Some(Value::Sequence(_))) {
        if matches!(m.get(&k), Some(v) if !v.is_null()) {
            bail!("descriptor/module.yaml: `{key}` must be a list");
        }
        m.insert(k.clone(), Value::Sequence(vec![]));
    }
    match m.get_mut(&k) {
        Some(Value::Sequence(s)) => Ok(s),
        _ => unreachable!(),
    }
}

fn finish(m: Mapping) -> Result<String> {
    let text = serde_yaml::to_string(&Value::Mapping(m))?;
    parse_source(&text).context("edit refused: the result would not parse")?;
    Ok(text)
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// Add `- name: <entity>` (owner of permissions/labels/capabilities) if absent.
pub fn add_entity(text: &str, entity: &str) -> Result<Option<String>> {
    let mut m = load(text)?;
    let list = seq(&mut m, "entities")?;
    if list.iter().any(|e| str_of(e, "name") == Some(entity)) {
        return Ok(None);
    }
    let mut e = Mapping::new();
    e.insert("name".into(), entity.into());
    list.push(Value::Mapping(e));
    finish(m).map(Some)
}

/// Add a link (one per `link_type`, `source`, `target`); `None` if it already exists.
pub fn add_link(text: &str, link: &LinkDesc) -> Result<Option<String>> {
    let mut m = load(text)?;
    let list = seq(&mut m, "links")?;
    if list.iter().any(|l| {
        str_of(l, "link_type") == Some(&link.link_type)
            && str_of(l, "source") == Some(&link.source)
            && str_of(l, "target") == Some(&link.target)
    }) {
        return Ok(None);
    }
    list.push(serde_yaml::to_value(link)?);
    finish(m).map(Some)
}

/// Add or replace a requirement on another module.
pub fn add_requirement(text: &str, req: &ModuleReq) -> Result<String> {
    let mut m = load(text)?;
    let list = seq(&mut m, "requires")?;
    list.retain(|r| str_of(r, "name") != Some(&req.name));
    list.push(serde_yaml::to_value(req)?);
    finish(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_unknown_keys_and_refuse_broken_results() {
        let src = "format: 1\nname: shop\nversion: 1.0.0\nx_future: { keep: me }\nentities:\n  - name: order\n    permissions: { list: public }\n";
        let out = add_entity(src, "invoice").unwrap().unwrap();
        let v: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["x_future"]["keep"], Value::from("me"));
        assert_eq!(
            v["entities"][0]["permissions"]["list"],
            Value::from("public")
        );
        assert_eq!(v["entities"][1]["name"], Value::from("invoice"));
        assert!(add_entity(&out, "invoice").unwrap().is_none(), "idempotent");

        let link = LinkDesc::new("has_invoice", "order", "invoice");
        let out2 = add_link(&out, &link).unwrap().unwrap();
        assert!(add_link(&out2, &link).unwrap().is_none());
        assert_eq!(parse_source(&out2).unwrap().links[0].target, "invoice");

        let bad = "format: 1\nname: shop\nversion: 1.0.0\nentities:\n  - name: order\n    permissions: { list: publik }\n";
        assert!(add_entity(bad, "x").is_err(), "unknown policy refused");
    }

    #[test]
    fn contract_without_params_has_no_suffix() {
        let e = ContractError::new("shop.boom", this_descriptor::ErrorKind::BadRequest);
        let shown = format!("{e}");
        let msg = format!("{}", contract(e));
        assert_eq!(msg, shown);
        assert!(!msg.ends_with(')'), "{msg}");
    }

    #[test]
    fn contract_renders_string_and_non_string_params() {
        let mut e = ContractError::new("shop.boom", this_descriptor::ErrorKind::BadRequest);
        e.params.insert("field".into(), serde_json::json!("email"));
        e.params.insert("max".into(), serde_json::json!(150));
        let shown = format!("{e}");
        let msg = format!("{}", contract(e));
        assert_eq!(msg, format!("{shown} (field: email; max: 150)"));
    }

    #[test]
    fn load_accepts_empty_document_and_rejects_non_mapping() {
        // An empty document is an empty mapping: the edit then fails on the parse
        // (no name/version), not on `load`.
        let err = add_entity("", "order").unwrap_err();
        assert!(
            format!("{err:#}").contains("edit refused: the result would not parse"),
            "{err:#}"
        );
        let err = add_entity("- just\n- a list\n", "order").unwrap_err();
        assert_eq!(format!("{err}"), "descriptor/module.yaml must be a mapping");
        let err = add_entity("a: [unclosed", "order").unwrap_err();
        assert_eq!(format!("{err}"), "descriptor/module.yaml is not YAML");
    }

    #[test]
    fn non_list_section_is_refused_and_null_section_is_replaced() {
        let base = "format: 1\nname: shop\nversion: 1.0.0\n";
        let err = add_entity(&format!("{base}entities: nope\n"), "order").unwrap_err();
        assert_eq!(
            format!("{err}"),
            "descriptor/module.yaml: `entities` must be a list"
        );
        let out = add_entity(&format!("{base}entities:\n"), "order")
            .unwrap()
            .unwrap();
        assert_eq!(parse_source(&out).unwrap().entities[0].name, "order");
    }

    #[test]
    fn add_requirement_replaces_existing_requirement_by_name() {
        let base = "format: 1\nname: shop\nversion: 1.0.0\nentities: []\n";
        let first = add_requirement(
            base,
            &ModuleReq {
                name: "users".into(),
                version: "^1.0.0".into(),
                optional: false,
            },
        )
        .unwrap();
        let second = add_requirement(
            &first,
            &ModuleReq {
                name: "users".into(),
                version: "^2.0.0".into(),
                optional: true,
            },
        )
        .unwrap();
        let d = parse_source(&second).unwrap();
        assert_eq!(d.requires.len(), 1);
        assert_eq!(d.requires[0].version, "^2.0.0");
        assert!(d.requires[0].optional);
    }

    #[test]
    fn initial_source_parses() {
        let d = parse_source(&initial_source("shop", "1.0.0", &["order".into()])).unwrap();
        assert_eq!(d.entities[0].name, "order");
        assert_eq!(d.entities[0].plural, "orders");
    }
}
