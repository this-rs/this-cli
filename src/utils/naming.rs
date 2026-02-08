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
    s.split(|c: char| c == '_' || c == '-' || c == ' ')
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

/// Basic English pluralization
/// "product" -> "products"
/// "category" -> "categories"
/// "status" -> "statuses"
/// "entity" -> "entities"
pub fn pluralize(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }

    // Already plural common cases
    if s.ends_with('s') && !s.ends_with("ss") && !s.ends_with("us") {
        return s.to_string();
    }

    if s.ends_with('y') {
        let prefix = &s[..s.len() - 1];
        // Check if the character before 'y' is a consonant
        if let Some(c) = prefix.chars().last() {
            if !"aeiou".contains(c) {
                return format!("{}ies", prefix);
            }
        }
    }

    if s.ends_with('s')
        || s.ends_with('x')
        || s.ends_with('z')
        || s.ends_with("sh")
        || s.ends_with("ch")
    {
        return format!("{}es", s);
    }

    format!("{}s", s)
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
}
