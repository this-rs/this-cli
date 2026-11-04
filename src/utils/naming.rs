/// Keywords that cannot be used as a Rust module, type or crate identifier.
/// Strict, reserved and 2018+ keywords, all lowercase (names are normalised first).
const RUST_KEYWORDS: &[&str] = &[
    "abstract", "as", "async", "await", "become", "box", "break", "const", "continue", "crate",
    "do", "dyn", "else", "enum", "extern", "false", "final", "fn", "for", "gen", "if", "impl",
    "in", "let", "loop", "macro", "match", "mod", "move", "mut", "override", "priv", "pub", "ref",
    "return", "self", "static", "struct", "super", "trait", "true", "try", "type", "typeof",
    "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

/// Normalise a user-supplied entity or project name and check that it is a safe
/// identifier for generated code and paths.
///
/// Rejects, with an error naming `what` and the offending input: empty names,
/// anything containing `/`, `\` or `.` (path separators, `..`, absolute paths), and
/// names that are not `^[a-z][a-z0-9_]*$` after [`to_snake_case`], or that are Rust
/// keywords. Returns the normalised name.
pub fn checked_name(raw: &str, what: &str) -> anyhow::Result<String> {
    if raw.trim().is_empty() {
        anyhow::bail!("{what} name cannot be empty");
    }
    if raw.contains(['/', '\\', '.']) {
        anyhow::bail!(
            "invalid {what} name '{raw}': path separators, '.' and absolute paths are not allowed"
        );
    }
    let normalized = to_snake_case(raw);
    let mut chars = normalized.chars();
    let well_formed = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !well_formed {
        anyhow::bail!(
            "invalid {what} name '{raw}': after normalisation it must match ^[a-z][a-z0-9_]*$ \
             (got '{normalized}')"
        );
    }
    if RUST_KEYWORDS.contains(&normalized.as_str()) {
        anyhow::bail!("invalid {what} name '{raw}': '{normalized}' is a Rust keyword");
    }
    Ok(normalized)
}

/// Convert a string to snake_case
/// "ProductCategory" -> "product_category"
/// "product-category" -> "product_category"
/// "product category" -> "product_category"
pub fn to_snake_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len() + 4);
    let mut prev_was_upper = false;
    let mut prev_was_separator = false;

    for (i, &c) in chars.iter().enumerate() {
        if c == '-' || c == '_' || c == ' ' {
            if !result.is_empty() {
                result.push('_');
            }
            prev_was_separator = true;
            prev_was_upper = false;
            continue;
        }

        if c.is_uppercase() {
            // Insert underscore before:
            // - A capital letter preceded by a lowercase letter: "myAPI" -> "my_api"
            // - A capital letter in a run of capitals followed by a lowercase: "HTMLParser" -> "html_parser"
            if i > 0 && !prev_was_separator && !result.is_empty() {
                let next_is_lower = chars.get(i + 1).is_some_and(|nc| nc.is_lowercase());
                if !prev_was_upper || next_is_lower {
                    result.push('_');
                }
            }
            result.push(c.to_lowercase().next().unwrap());
            prev_was_upper = true;
        } else {
            result.push(c);
            prev_was_upper = false;
        }
        prev_was_separator = false;
    }

    result
}

/// Convert a string to PascalCase
/// "product_category" -> "ProductCategory"
/// "product-category" -> "ProductCategory"
/// "product" -> "Product"
pub fn to_pascal_case(s: &str) -> String {
    s.split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let upper: String = first.to_uppercase().collect();
                    upper + chars.as_str()
                }
                None => String::new(),
            }
        })
        .collect()
}

/// Plural of an entity name.
///
/// Delegates to `this_descriptor::naming::Pluralizer`, the ONE pluralizer shared with the
/// `impl_data_entity!` macro, `EntityDesc::normalize` and `Entity::resource_name()`. The
/// former CLI algorithm diverged (knife -> knifes here, knives at runtime), which made
/// generated routes disagree with the server.
pub fn pluralize(s: &str) -> String {
    this_descriptor::naming::Pluralizer::pluralize(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("Product"), "product");
        assert_eq!(to_snake_case("ProductCategory"), "product_category");
        assert_eq!(to_snake_case("product-category"), "product_category");
        assert_eq!(to_snake_case("product_category"), "product_category");
        assert_eq!(to_snake_case("HTMLParser"), "html_parser");
        assert_eq!(to_snake_case("myAPI"), "my_api");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("product"), "Product");
        assert_eq!(to_pascal_case("product_category"), "ProductCategory");
        assert_eq!(to_pascal_case("product-category"), "ProductCategory");
        assert_eq!(to_pascal_case("stock_item"), "StockItem");
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize("product"), "products");
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("entity"), "entities");
        assert_eq!(pluralize("status"), "statuses");
        assert_eq!(pluralize("tax"), "taxes");
        assert_eq!(pluralize("key"), "keys");
        assert_eq!(pluralize("day"), "days");
        assert_eq!(pluralize("bush"), "bushes");
        assert_eq!(pluralize("match"), "matches");
    }

    #[test]
    fn checked_name_accepts_identifiers_and_normalises_them() {
        assert_eq!(checked_name("product", "entity").unwrap(), "product");
        assert_eq!(
            checked_name("ProductCategory", "entity").unwrap(),
            "product_category"
        );
        assert_eq!(checked_name("my-project", "project").unwrap(), "my_project");
        assert_eq!(checked_name("item2", "entity").unwrap(), "item2");
    }

    #[test]
    fn checked_name_refuses_path_like_names() {
        for bad in ["../evil", "a/b", "a\\b", "/etc/passwd", "x.y", "."] {
            let err = checked_name(bad, "entity").unwrap_err().to_string();
            assert!(err.contains("invalid entity name"), "{bad}: {err}");
        }
    }

    #[test]
    fn checked_name_refuses_non_identifiers_and_keywords() {
        for bad in ["2fast", "émoji", "a b c!", "type", "match", "Self"] {
            assert!(
                checked_name(bad, "project").is_err(),
                "{bad} must be refused"
            );
        }
        assert!(checked_name("", "entity").is_err());
        assert!(checked_name("   ", "entity").is_err());
    }

    #[test]
    fn checked_name_refuses_every_rust_keyword_in_any_case() {
        for kw in RUST_KEYWORDS {
            let err = checked_name(kw, "entity").unwrap_err().to_string();
            assert!(err.contains("is a Rust keyword"), "{kw}: {err}");
            let shouted = kw[..1].to_uppercase() + &kw[1..];
            assert!(
                checked_name(&shouted, "entity").is_err(),
                "{shouted} normalises to a keyword and must be refused"
            );
        }
    }

    #[test]
    fn cli_uses_shared_pluralizer() {
        for word in [
            "knife", "leaf", "person", "company", "address", "status", "box", "news",
        ] {
            assert_eq!(
                pluralize(word),
                this_descriptor::naming::Pluralizer::pluralize(word),
                "CLI plural of `{word}` must be the runtime plural"
            );
        }
        assert_eq!(pluralize("knife"), "knives");
    }
}
