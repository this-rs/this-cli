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

    fn err_chain<T>(r: Result<T>) -> String {
        match r {
            Ok(_) => panic!("expected an error"),
            Err(e) => format!("{e:#}"),
        }
    }

    const PARTIAL_MODEL: &str = r#"
impl_data_entity_validated!(
    Order,
    "order",
    ["name"],
    {
        // a comment entry,
        tenant_id: Uuid,
        loose,
        sku: String,
        qty: Option<i32>,
    },
    validate: {
        create: { sku: [required string_length(2, 8)] },
    },
    filters: {
        update: { sku: [trim] },
    }
);
"#;

    #[test]
    fn braced_after_is_quote_aware_and_none_when_unterminated() {
        let s = r#"x { "a}b" { y } } tail"#;
        let (a, b) = braced_after(s, 0).unwrap();
        assert_eq!(&s[a..b], r#" "a}b" { y } "#);
        assert_eq!(braced_after("x { never closed", 0), None);
        assert_eq!(braced_after("no brace here", 0), None);
    }

    #[test]
    fn rule_tokens_parse_args_and_reject_garbage() {
        let t = rule_tokens(r#"required, string_length(3, 50) in_list("a,b", "c\"d")"#).unwrap();
        assert_eq!(t[0].name, "required");
        assert!(t[0].args.is_empty());
        assert_eq!(t[1].name, "string_length");
        assert_eq!(t[1].args, vec!["3", "50"]);
        assert_eq!(t[2].name, "in_list");
        assert_eq!(t[2].args, vec!["a,b", "c\"d"]);
        let e = err_chain(rule_tokens("required %"));
        assert!(e.contains("unexpected `%` in rule list"), "{e}");
    }

    #[test]
    fn macro_validator_from_covers_grammar_and_rejects_the_rest() {
        let tok = |name: &str, args: &[&str]| RuleToken {
            name: name.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            on: None,
        };
        assert_eq!(
            macro_validator_from(&tok("positive", &[])).unwrap(),
            ValidatorSpec::Positive
        );
        assert_eq!(
            macro_validator_from(&tok("max_value", &["10f64"])).unwrap(),
            ValidatorSpec::max_value_f64(10.0)
        );
        assert_eq!(
            macro_validator_from(&tok("date_format", &["%Y"])).unwrap(),
            ValidatorSpec::DateFormat("%Y".into())
        );
        assert_eq!(
            macro_validator_from(&tok("date_format", &[])).unwrap(),
            ValidatorSpec::DateFormat(String::new())
        );
        let e = err_chain(macro_validator_from(&tok("email", &[])));
        assert!(
            e.contains("validator `email` is not in the impl_data_entity_validated! grammar"),
            "{e}"
        );
        let e = err_chain(macro_validator_from(&tok("string_length", &["3"])));
        assert!(e.contains("`string_length` needs a number"), "{e}");
    }

    #[test]
    fn macro_transform_from_covers_grammar_and_rejects_the_rest() {
        let tok = |name: &str, args: &[&str]| RuleToken {
            name: name.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            on: None,
        };
        assert_eq!(
            macro_transform_from(&tok("uppercase", &[])).unwrap(),
            TransformSpec::Uppercase
        );
        assert_eq!(
            macro_transform_from(&tok("round_decimals", &["4"])).unwrap(),
            TransformSpec::RoundDecimals(4)
        );
        let e = err_chain(macro_transform_from(&tok("round_decimals", &[])));
        assert!(e.contains("round_decimals(n)"), "{e}");
        let e = err_chain(macro_transform_from(&tok("reverse", &[])));
        assert!(
            e.contains("filter `reverse` is not in the impl_data_entity_validated! grammar"),
            "{e}"
        );
    }

    #[test]
    fn rule_lines_reports_unterminated_list() {
        let ok = rule_lines("a: [required], b: [trim]").unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[1].0, "b");
        assert_eq!(ok[1].1[0].name, "trim");
        let e = err_chain(rule_lines("a: [required"));
        assert!(e.contains("unterminated rule list"), "{e}");
    }

    #[test]
    fn entity_from_model_reads_partial_validated_blocks() {
        let e = entity_from_model(PARTIAL_MODEL).unwrap().unwrap();
        let names: Vec<&str> = e.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(
            names.contains(&"sku") && names.contains(&"qty"),
            "{names:?}"
        );
        assert!(!names.contains(&"loose"), "entry without a type is skipped");
        assert!(!names.iter().any(|n| n.starts_with("//")), "{names:?}");
        assert_eq!(e.indexed, vec!["name".to_string()]);
        let tenant = e.fields.iter().find(|f| f.name == "tenant_id").unwrap();
        assert!(tenant.read_only, "tenant_id is read-only by the macro");
        let qty = e.fields.iter().find(|f| f.name == "qty").unwrap();
        assert!(qty.optional);
        assert_eq!(qty.ty, this_descriptor::FieldType::I32);
        let sku = e.fields.iter().find(|f| f.name == "sku").unwrap();
        // `validate` has a create block only, `filters` an update block only.
        assert!(
            sku.validators
                .iter()
                .any(|v| v.rule == ValidatorSpec::Required && v.on == Applies::Create)
        );
        assert!(
            sku.validators
                .iter()
                .any(|v| v.rule == (ValidatorSpec::StringLength { min: 2, max: 8 }))
        );
        assert!(sku.validators.iter().all(|v| v.on == Applies::Create));
        assert_eq!(sku.transforms.len(), 1);
        assert_eq!(sku.transforms[0].rule, TransformSpec::Trim);
        assert_eq!(sku.transforms[0].on, Applies::Update);
    }

    #[test]
    fn entity_from_model_without_validate_or_filters_sections() {
        let src = r#"impl_data_entity_validated!(Order, "order", [], { sku: String });"#;
        let e = entity_from_model(src).unwrap().unwrap();
        assert!(e.indexed.is_empty());
        let sku = e.fields.iter().find(|f| f.name == "sku").unwrap();
        assert!(sku.validators.is_empty() && sku.transforms.is_empty());
    }

    #[test]
    fn entity_from_model_none_without_macro() {
        assert!(entity_from_model("pub struct Plain;").unwrap().is_none());
    }

    #[test]
    fn entity_from_model_errors() {
        let e = err_chain(entity_from_model(
            r#"impl_data_entity!(Order, "order", ["name"], { sku: String"#,
        ));
        assert!(e.contains("fields block of the macro"), "{e}");

        let e = err_chain(entity_from_model(
            r#"impl_data_entity_validated!(Order, "order", [], { sku: String },
               validate: { create: { ghost: [required] } });"#,
        ));
        assert!(
            e.contains("rule on `ghost`, neither a declared field nor name/status"),
            "{e}"
        );

        let e = err_chain(entity_from_model(
            r#"impl_data_entity_validated!(Order, "order", [], { sku: String },
               validate: { create: { sku: [email] } });"#,
        ));
        assert!(e.contains("validator `email` is not in"), "{e}");

        let e = err_chain(entity_from_model(
            r##"impl_data_entity!(Order, "order", [], { sku: String });
               const DESCRIBED_EXTRA: &str = r#"not json"#;"##,
        ));
        assert!(
            e.contains("DESCRIBED_EXTRA of `order` is not valid JSON"),
            "{e}"
        );
    }

    #[test]
    fn merge_extra_adds_rules_and_refuses_unknown_fields() {
        let mut e = EntityDesc::new("order");
        e.fields
            .push(FieldDesc::new("sku", this_descriptor::FieldType::String));
        let mut x = FieldDesc::new("sku", this_descriptor::FieldType::String);
        x.validators.push(ValidatorRule {
            rule: ValidatorSpec::Email,
            on: Applies::Always,
        });
        x.sortable = true;
        x.immutable = true;
        merge_extra(&mut e, vec![x]).unwrap();
        let sku = e.fields.iter().find(|f| f.name == "sku").unwrap();
        assert_eq!(sku.validators.len(), 1);
        assert_eq!(sku.validators[0].rule, ValidatorSpec::Email);
        assert!(sku.sortable && sku.immutable && !sku.read_only);

        let ghost = FieldDesc::new("ghost", this_descriptor::FieldType::String);
        let e2 = err_chain(merge_extra(&mut e, vec![ghost]));
        assert!(
            e2.contains(
                "DESCRIBED_EXTRA of `order`: `ghost` is neither a declared field nor name/status"
            ),
            "{e2}"
        );
    }

    #[test]
    fn described_extra_is_read_with_any_hash_count() {
        let src = "const DESCRIBED_EXTRA: &str = r##\"[{\"a\":\"x\\\"#\"}]\"##;";
        assert_eq!(
            described_extra(src).as_deref(),
            Some("[{\"a\":\"x\\\"#\"}]")
        );
        assert_eq!(described_extra("const OTHER: &str = \"\";"), None);
        // Opening without its closing quote+hashes: no value.
        assert_eq!(described_extra("DESCRIBED_EXTRA: &str = r#\"abc"), None);
    }
}
