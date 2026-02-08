use anyhow::Result;

/// Insert a line after a marker comment in the content.
/// The marker is a comment like `// [this:store_fields]`.
/// The new line is inserted on the next line after the marker, with the same indentation.
///
/// Returns an error if the marker is not found.
pub fn insert_after_marker(content: &str, marker: &str, line: &str) -> Result<String> {
    let mut lines: Vec<&str> = content.lines().collect();
    let marker_idx = lines
        .iter()
        .position(|l| l.trim().contains(marker))
        .ok_or_else(|| anyhow::anyhow!("Marker '{}' not found in file", marker))?;

    // Detect indentation from the marker line
    let indent =
        &lines[marker_idx][..lines[marker_idx].len() - lines[marker_idx].trim_start().len()];
    let indented_line = format!("{}{}", indent, line);

    lines.insert(marker_idx + 1, &indented_line);

    // Rebuild with trailing newline
    let mut result = lines.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

/// Check if a line already exists after a marker (for idempotence).
/// Searches all lines between the marker and the end of the file (or next marker).
pub fn has_line_after_marker(content: &str, marker: &str, needle: &str) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    let marker_idx = match lines.iter().position(|l| l.trim().contains(marker)) {
        Some(idx) => idx,
        None => return false,
    };

    // Search lines after the marker
    for line in &lines[marker_idx + 1..] {
        if line.trim().contains(needle) {
            return true;
        }
    }
    false
}

/// Add a use import line to the file if it doesn't already exist.
/// Inserts after the last existing `use` line, or at the top if none found.
pub fn add_import(content: &str, import_line: &str) -> String {
    // Check if import already exists
    if content.lines().any(|l| l.trim() == import_line.trim()) {
        return content.to_string();
    }

    let lines: Vec<&str> = content.lines().collect();

    // Find the last `use` line
    let last_use_idx = lines
        .iter()
        .rposition(|l| l.trim_start().starts_with("use "));

    let insert_idx = match last_use_idx {
        Some(idx) => idx + 1,
        None => 0,
    };

    let mut result_lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    result_lines.insert(insert_idx, import_line.to_string());

    let mut result = result_lines.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_after_marker_basic() {
        let content = "pub struct Stores {\n    // [this:store_fields]\n}\n";
        let result = insert_after_marker(
            content,
            "[this:store_fields]",
            "pub product_store: Arc<dyn ProductStore>,",
        )
        .unwrap();
        assert!(result.contains("pub product_store: Arc<dyn ProductStore>,"));
        assert!(result.contains("// [this:store_fields]"));
    }

    #[test]
    fn test_insert_after_marker_preserves_indent() {
        let content = "impl Stores {\n    pub fn new() -> Self {\n        // [this:store_init_fields]\n    }\n}\n";
        let result = insert_after_marker(
            content,
            "[this:store_init_fields]",
            "product_store: products.clone(),",
        )
        .unwrap();
        assert!(result.contains("        product_store: products.clone(),"));
    }

    #[test]
    fn test_insert_after_marker_not_found() {
        let content = "pub struct Stores {}\n";
        let result = insert_after_marker(content, "[this:nonexistent]", "something");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_has_line_after_marker_found() {
        let content = "pub struct Stores {\n    // [this:store_fields]\n    pub product_store: Arc<dyn ProductStore>,\n}\n";
        assert!(has_line_after_marker(
            content,
            "[this:store_fields]",
            "product_store"
        ));
    }

    #[test]
    fn test_has_line_after_marker_not_found() {
        let content = "pub struct Stores {\n    // [this:store_fields]\n}\n";
        assert!(!has_line_after_marker(
            content,
            "[this:store_fields]",
            "product_store"
        ));
    }

    #[test]
    fn test_has_line_after_marker_no_marker() {
        let content = "pub struct Stores {}\n";
        assert!(!has_line_after_marker(
            content,
            "[this:store_fields]",
            "product_store"
        ));
    }

    #[test]
    fn test_idempotent_insert() {
        let content = "pub struct Stores {\n    // [this:store_fields]\n    pub product_store: Arc<dyn ProductStore>,\n}\n";
        // Should detect that product_store already exists
        assert!(has_line_after_marker(
            content,
            "[this:store_fields]",
            "product_store"
        ));
        // So we wouldn't insert again
    }

    #[test]
    fn test_add_import_basic() {
        let content = "use std::sync::Arc;\n\npub struct Foo;\n";
        let result = add_import(content, "use super::product::ProductStore;");
        assert!(result.contains("use super::product::ProductStore;"));
        assert!(result.contains("use std::sync::Arc;"));
    }

    #[test]
    fn test_add_import_no_existing_imports() {
        let content = "pub struct Foo;\n";
        let result = add_import(content, "use std::sync::Arc;");
        assert!(result.starts_with("use std::sync::Arc;"));
    }

    #[test]
    fn test_add_import_already_exists() {
        let content = "use std::sync::Arc;\n\npub struct Foo;\n";
        let result = add_import(content, "use std::sync::Arc;");
        // Should not duplicate
        assert_eq!(result.matches("use std::sync::Arc;").count(), 1);
    }

    #[test]
    fn test_add_import_after_last_use() {
        let content = "use std::sync::Arc;\nuse anyhow::Result;\n\npub struct Foo;\n";
        let result = add_import(content, "use super::product::ProductStore;");
        // The import should be after "use anyhow::Result;"
        let lines: Vec<&str> = result.lines().collect();
        let arc_idx = lines.iter().position(|l| l.contains("Arc")).unwrap();
        let anyhow_idx = lines.iter().position(|l| l.contains("anyhow")).unwrap();
        let product_idx = lines
            .iter()
            .position(|l| l.contains("ProductStore"))
            .unwrap();
        assert!(product_idx > anyhow_idx);
        assert!(product_idx > arc_idx);
    }
}
