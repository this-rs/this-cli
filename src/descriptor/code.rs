//! Read an entity's descriptor back from its generated code, the way the this-rs macros
//! build it (`__this_described!`): `impl_data_entity(_validated)!` gives name, plural,
//! indexed fields, field types and the rules of its grammar; the `DESCRIBED_EXTRA` literal
//! (written by `this add entity`) gives the rules the grammar cannot carry.
//!
//! This replaces the third copy of the macro grammar (regex-only `introspect.rs`, brace
//! parser of `info.rs`): one reader, and a test checks it against the renderer.

use anyhow::{Context, Result, bail};
use this_descriptor::{
    Applies, EntityDesc, FieldDesc, TransformRule, TransformSpec, ValidatorRule, ValidatorSpec,
};

use super::from_fields::{RuleToken, probe_rust_type, split_top};

/// Content between the `{` following `start` and its matching `}` (quote-aware).
fn braced_after(s: &str, start: usize) -> Option<(usize, usize)> {
    let open = start + s[start..].find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut prev = '\0';
    for (i, c) in s[open..].char_indices() {
        if in_str {
            if c == '"' && prev != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((open + 1, open + i));
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

/// Inner text of `key: { ... }` inside `s`, searching from `from`.
fn block<'a>(s: &'a str, key: &str, from: usize) -> Option<&'a str> {
    let re = regex::Regex::new(&format!(r"\b{}\s*:\s*\{{", regex::escape(key))).ok()?;
    let m = re.find_at(s, from)?;
    let (a, b) = braced_after(s, m.start())?;
    Some(&s[a..b])
}

/// Tokens of `[required string_length(3, 50) in_list("a", "b")]`.
fn rule_tokens(list: &str) -> Result<Vec<RuleToken>> {
    let mut out = Vec::new();
    let chars: Vec<char> = list.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() || chars[i] == ',' {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        if start == i {
            bail!("unexpected `{}` in rule list `{list}`", chars[i]);
        }
        let name: String = chars[start..i].iter().collect();
        let mut args = vec![];
        if i < chars.len() && chars[i] == '(' {
            let mut depth = 0;
            let mut in_str = false;
            let a = i + 1;
            while i < chars.len() {
                let c = chars[i];
                if in_str {
                    if c == '"' && chars[i - 1] != '\\' {
                        in_str = false;
                    }
                } else if c == '"' {
                    in_str = true;
                } else if c == '(' {
                    depth += 1;
                } else if c == ')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                i += 1;
            }
            let inner: String = chars[a..i].iter().collect();
            i += 1;
            args = split_top(&inner, ',')
                .into_iter()
                .map(|s| {
                    let t = s.trim();
                    if t.starts_with('"') {
                        // Rust string literal -> value.
                        serde_json::from_str::<String>(t)
                            .unwrap_or_else(|_| t.trim_matches('"').into())
                    } else {
                        t.to_string()
                    }
                })
                .filter(|s| !s.is_empty())
                .collect();
        }
        out.push(RuleToken {
            name,
            args,
            on: None,
        });
    }
    Ok(out)
}

fn macro_validator_from(tok: &RuleToken) -> Result<ValidatorSpec> {
    let num = |i: usize| -> Result<f64> {
        tok.args
            .get(i)
            .and_then(|a| a.trim_end_matches("f64").trim_end_matches('.').parse().ok())
            .with_context(|| format!("`{}` needs a number", tok.name))
    };
    Ok(match tok.name.as_str() {
        "required" => ValidatorSpec::Required,
        "optional" => ValidatorSpec::Optional,
        "positive" => ValidatorSpec::Positive,
        "string_length" => ValidatorSpec::StringLength {
            min: num(0)? as usize,
            max: num(1)? as usize,
        },
        "max_value" => ValidatorSpec::max_value_f64(num(0)?),
        "in_list" => ValidatorSpec::in_list_strings(tok.args.clone()),
        "date_format" => ValidatorSpec::DateFormat(tok.args.first().cloned().unwrap_or_default()),
        other => bail!("validator `{other}` is not in the impl_data_entity_validated! grammar"),
    })
}

fn macro_transform_from(tok: &RuleToken) -> Result<TransformSpec> {
    Ok(match tok.name.as_str() {
        "trim" => TransformSpec::Trim,
        "uppercase" => TransformSpec::Uppercase,
        "lowercase" => TransformSpec::Lowercase,
        "round_decimals" => TransformSpec::RoundDecimals(
            tok.args
                .first()
                .and_then(|a| a.parse().ok())
                .context("round_decimals(n)")?,
        ),
        other => bail!("filter `{other}` is not in the impl_data_entity_validated! grammar"),
    })
}

/// `field: [rules],` lines of an operation block.
fn rule_lines(block: &str) -> Result<Vec<(String, Vec<RuleToken>)>> {
    let re = regex::Regex::new(r"(\w+)\s*:\s*\[").unwrap();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(c) = re.captures_at(block, pos) {
        let m = c.get(0).unwrap();
        let open = m.end();
        // matching `]` outside quotes
        let mut in_str = false;
        let mut prev = '\0';
        let mut close = None;
        for (i, ch) in block[open..].char_indices() {
            if in_str {
                if ch == '"' && prev != '\\' {
                    in_str = false;
                }
            } else if ch == '"' {
                in_str = true;
            } else if ch == ']' {
                close = Some(open + i);
                break;
            }
            prev = ch;
        }
        let close = close.context("unterminated rule list")?;
        out.push((c[1].to_string(), rule_tokens(&block[open..close])?));
        pos = close + 1;
    }
    Ok(out)
}

/// Value of `const DESCRIBED_EXTRA: &str = r#"..."#;` if present.
pub fn described_extra(content: &str) -> Option<String> {
    let re = regex::Regex::new(r#"DESCRIBED_EXTRA\s*:\s*&str\s*=\s*r(#+)""#).unwrap();
    let c = re.captures(content)?;
    let hashes = c[1].to_string();
    let start = c.get(0)?.end();
    let end = start + content[start..].find(&format!("\"{hashes}"))?;
    Some(content[start..end].to_string())
}

/// Merge the extra rules into the macro's descriptor: the SAME algorithm as the generated
/// `described()` (template `entity/model*.rs.tera`).
pub fn merge_extra(e: &mut EntityDesc, extra: Vec<FieldDesc>) -> Result<()> {
    for x in extra {
        let name = e.name.clone();
        let f = e.field_or_overlay_mut(&x.name).with_context(|| {
            format!(
                "DESCRIBED_EXTRA of `{name}`: `{}` is neither a declared field nor name/status",
                x.name
            )
        })?;
        f.validators.extend(x.validators);
        f.transforms.extend(x.transforms);
        for op in x.filterable {
            if !f.filterable.contains(&op) {
                f.filterable.push(op);
            }
        }
        f.sortable |= x.sortable;
        f.read_only |= x.read_only;
        f.immutable |= x.immutable;
    }
    e.normalize();
    Ok(())
}

/// Descriptor of the entity defined in `content` (a `model.rs`), or `None` when the file
/// has no `impl_data_entity(_validated)!`.
pub fn entity_from_model(content: &str) -> Result<Option<EntityDesc>> {
    let head = regex::Regex::new(
        r#"impl_data_entity(_validated)?!\(\s*(\w+)\s*,\s*"(\w+)"\s*,\s*\[([^\]]*)\]\s*,"#,
    )
    .unwrap();
    let Some(c) = head.captures(content) else {
        return Ok(None);
    };
    let validated = c.get(1).is_some();
    let type_name = c[3].to_string();
    let indexed: Vec<String> = regex::Regex::new(r#""([^"]+)""#)
        .unwrap()
        .captures_iter(&c[4])
        .map(|m| m[1].to_string())
        .collect();
    let after = c.get(0).unwrap().end();
    let (fa, fb) = braced_after(content, after).context("fields block of the macro")?;

    let mut e = EntityDesc::new(type_name.clone());
    e.indexed = indexed;
    for entry in split_top(&content[fa..fb], ',') {
        let entry = entry.trim();
        if entry.is_empty() || entry.starts_with("//") {
            continue;
        }
        let Some((name, ty)) = entry.split_once(':') else {
            continue;
        };
        let (ty, optional) = probe_rust_type(ty);
        let mut f = FieldDesc::new(name.trim(), ty);
        f.optional = optional;
        if f.name == this_descriptor::entity::TENANT_ID {
            f.read_only = true;
        }
        e.fields.push(f);
    }

    if validated {
        let rest = fb;
        for (key, is_validator) in [("validate", true), ("filters", false)] {
            let Some(section) = block(content, key, rest) else {
                continue;
            };
            for op_name in ["create", "update"] {
                let Some(ops) = block(section, op_name, 0) else {
                    continue;
                };
                let on = if op_name == "create" {
                    Applies::Create
                } else {
                    Applies::Update
                };
                for (field, toks) in rule_lines(ops)? {
                    let f = e.field_or_overlay_mut(&field).with_context(|| {
                        format!("rule on `{field}`, neither a declared field nor name/status")
                    })?;
                    for t in &toks {
                        if is_validator {
                            f.validators.push(ValidatorRule {
                                rule: macro_validator_from(t)?,
                                on,
                            });
                        } else {
                            f.transforms.push(TransformRule {
                                rule: macro_transform_from(t)?,
                                on,
                            });
                        }
                    }
                }
            }
        }
    }
    e.normalize();

    if let Some(json) = described_extra(content) {
        let extra: Vec<FieldDesc> = serde_json::from_str(&json)
            .with_context(|| format!("DESCRIBED_EXTRA of `{type_name}` is not valid JSON"))?;
        merge_extra(&mut e, extra)?;
    }
    Ok(Some(e))
}

/// Sort rule lists so that two descriptors built in a different order compare equal.
pub fn canonical(e: &EntityDesc) -> EntityDesc {
    let mut e = e.clone();
    let key = |v: &serde_json::Value| v.to_string();
    for f in &mut e.fields {
        f.validators
            .sort_by_key(|r| key(&serde_json::to_value(r).unwrap_or_default()));
        f.transforms
            .sort_by_key(|r| key(&serde_json::to_value(r).unwrap_or_default()));
        f.filterable.sort_by_key(|o| o.as_str());
        f.proto_tag = None;
    }
    e.fields.sort_by(|a, b| a.name.cmp(&b.name));
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::from_fields::{
        ModelRender, apply_legacy_validated_defaults, parse_fields,
    };
    use crate::templates::TemplateEngine;

    /// Render `entity/model*.rs` from an `EntityDesc` exactly as `this add entity` does.
    fn render(e: &EntityDesc, described: bool) -> String {
        let model = ModelRender::from_entity(e).unwrap();
        let mut ctx = tera::Context::new();
        let pascal = crate::utils::naming::to_pascal_case(&e.name);
        ctx.insert("entity_name", &e.name);
        ctx.insert("entity_pascal", &pascal);
        ctx.insert("entity_plural", &e.plural);
        ctx.insert("fields", &model.fields);
        ctx.insert("indexed_fields", &model.indexed_fields);
        ctx.insert("validated", &model.validated);
        ctx.insert("entity", e);
        ctx.insert("model", &model);
        ctx.insert("described", &described);
        let tpl = if model.validated {
            "entity/model_validated.rs"
        } else {
            "entity/model.rs"
        };
        TemplateEngine::new().unwrap().render(tpl, &ctx).unwrap()
    }

    fn entity(fields: &str) -> EntityDesc {
        let mut e = EntityDesc::new("customer");
        e.indexed = vec!["name".into(), "email".into()];
        e.fields = parse_fields(fields).unwrap();
        e
    }

    #[test]
    fn templates_render_from_descriptor() {
        let e = entity(
            "email:String:required:email:trim,score:Option<f64>:min_value(0.5):max_value(150)@create,\
             code:String:string_length(2,8):uppercase:filter(eq):sortable,\
             kind:String:in_list(a|b)",
        );
        let out = render(&e, true);
        // Struct fields and types come from the descriptor.
        assert!(out.contains("impl_data_entity_validated!("), "{out}");
        assert!(out.contains("        email: String,"));
        assert!(out.contains("        score: Option<f64>,"));
        assert!(out.contains("[\"name\", \"email\"]"));
        // Macro-grammar rules, per operation (an `always` rule in both blocks).
        assert!(out.contains("email: [required],"));
        assert!(out.contains("score: [max_value(150)],"));
        assert!(out.contains("code: [string_length(2, 8)],"));
        assert!(out.contains("kind: [in_list(\"a\", \"b\")],"));
        assert!(out.contains("email: [trim],"));
        assert!(out.contains("code: [uppercase],"));
        // The rest goes to DESCRIBED_EXTRA, read by described().
        let extra = described_extra(&out).expect("DESCRIBED_EXTRA");
        let extra: Vec<FieldDesc> = serde_json::from_str(&extra).unwrap();
        let names: Vec<&str> = extra.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["email", "score", "code"]);
        assert!(out.contains("pub fn described() -> this::descriptor::EntityDesc"));
        assert!(
            !out.contains("{{") && !out.contains("{%"),
            "no Tera left:\n{out}"
        );

        // Without descriptor mode and without extra rule: no described().
        let plain = render(&entity("sku:String,price:f64"), false);
        assert!(plain.contains("impl_data_entity!("));
        assert!(!plain.contains("described"));
    }

    #[test]
    fn generated_descriptor_matches_code() {
        for fields in [
            "sku:String,price:f64,note:Option<String>",
            "email:String:required:email:trim,age:Option<i32>:min_value(18):max_value(150)@create",
            "code:String:string_length(2,8):uppercase:lowercase@update:filter(eq,in):sortable,\
             ref_no:String:pattern(\"^[A-Z]{2}-\\\\d+$\"):date_format(\"%Y\"),\
             total:f64:positive:round_decimals(2):read_only,kind:String:in_list(x|y)",
        ] {
            let mut e = entity(fields);
            e.normalize();
            let back = entity_from_model(&render(&e, true)).unwrap().unwrap();
            assert_eq!(canonical(&back), canonical(&e), "fields: {fields}");
            assert_eq!(back.plural, "customers");
            assert_eq!(back.indexed, e.indexed);
        }
        // Legacy --validated defaults, base-field overlay (`status`) included.
        let mut e = entity("sku:String,price:f64,note:Option<String>");
        apply_legacy_validated_defaults(&mut e);
        e.normalize();
        let out = render(&e, false);
        assert!(
            described_extra(&out).is_none(),
            "legacy rules all fit the macro:\n{out}"
        );
        let back = entity_from_model(&out).unwrap().unwrap();
        assert_eq!(canonical(&back), canonical(&e));
    }
}
