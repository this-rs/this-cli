//! Project introspection — parse entities, fields, descriptors, and links
//! from a this-rs project source tree to extract metadata for code generation.

use anyhow::{Context, Result};
use regex::Regex;
use std::path::Path;

// ── Metadata structs ──────────────────────────────────────────────────

/// A single field on an entity.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldMeta {
    /// Field name (snake_case)
    pub name: String,
    /// Rust type as a string, e.g. `String`, `f64`, `Option<String>`
    pub rust_type: String,
}

/// REST route info extracted from a descriptor.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteMeta {
    /// HTTP method (GET, POST, PUT, DELETE)
    pub method: String,
    /// Path pattern, e.g. `/products` or `/products/{id}`
    pub path: String,
}

/// Full metadata for a single entity.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityMeta {
    /// PascalCase name (e.g. `Product`)
    pub pascal_name: String,
    /// snake_case name (e.g. `product`)
    pub snake_name: String,
    /// Plural form (e.g. `products`)
    pub plural: String,
    /// Indexed fields (from `impl_data_entity!`)
    pub indexed_fields: Vec<String>,
    /// Entity fields (from `impl_data_entity!`)
    pub fields: Vec<FieldMeta>,
    /// REST routes (from descriptor)
    pub routes: Vec<RouteMeta>,
}

/// A typed link between two entity types.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkMeta {
    /// Link type name (e.g. `has_invoice`)
    pub link_type: String,
    /// Source entity (snake_case)
    pub source: String,
    /// Target entity (snake_case)
    pub target: String,
    /// Forward route name (e.g. `invoices`)
    pub forward_route: String,
    /// Reverse route name (e.g. `order`)
    pub reverse_route: String,
}

/// Complete project introspection result.
#[derive(Debug, Clone)]
pub struct ProjectIntrospection {
    /// All discovered entities with their fields and routes
    pub entities: Vec<EntityMeta>,
    /// All link definitions
    pub links: Vec<LinkMeta>,
}

// ── Parser implementation ─────────────────────────────────────────────

/// Introspect a this-rs project at `api_root` (the directory containing `src/` and `config/`).
///
/// Scans:
/// - `src/entities/*/model.rs` for entity definitions (`impl_data_entity!`)
/// - `src/entities/*/descriptor.rs` for REST routes
/// - `config/links.yaml` for link definitions
pub fn introspect(api_root: &Path) -> Result<ProjectIntrospection> {
    let entities_dir = api_root.join("src/entities");
    let links_path = api_root.join("config/links.yaml");

    let mut entities = Vec::new();

    if entities_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&entities_dir)
            .with_context(|| format!("Failed to read: {}", entities_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let entity_dir = entry.path();
            let model_path = entity_dir.join("model.rs");
            let descriptor_path = entity_dir.join("descriptor.rs");

            if model_path.exists() {
                let mut entity = parse_entity_model(&model_path)?;

                // Enrich with descriptor info (routes + plural)
                if descriptor_path.exists() {
                    let (plural, routes) = parse_descriptor(&descriptor_path)?;
                    if !plural.is_empty() {
                        entity.plural = plural;
                    }
                    entity.routes = routes;
                }

                entities.push(entity);
            }
        }
    }

    let links = if links_path.exists() {
        parse_links_yaml(&links_path)?
    } else {
        Vec::new()
    };

    Ok(ProjectIntrospection { entities, links })
}

// ── Entity model parser ───────────────────────────────────────────────

/// Parse `impl_data_entity!(PascalName, "snake_name", ["idx1", "idx2"], { field: Type, ... })`
/// from a model.rs file.
pub fn parse_entity_model(path: &Path) -> Result<EntityMeta> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;

    parse_entity_model_content(&content, path)
}

fn parse_entity_model_content(content: &str, path: &Path) -> Result<EntityMeta> {
    // Match: impl_data_entity!( or impl_data_entity_validated!(
    let macro_re = Regex::new(
        r#"impl_data_entity(?:_validated)?\!\(\s*(\w+)\s*,\s*"(\w+)"\s*,\s*\[([^\]]*)\]\s*,\s*\{([^}]*)\}"#
    ).unwrap();

    let caps = macro_re
        .captures(content)
        .ok_or_else(|| anyhow::anyhow!("No impl_data_entity! macro found in {}", path.display()))?;

    let pascal_name = caps[1].to_string();
    let snake_name = caps[2].to_string();

    // Parse indexed fields: "field1", "field2"
    let indexed_raw = caps[3].trim();
    let indexed_fields = parse_string_list(indexed_raw);

    // Parse fields block: field_name: Type,
    let fields_raw = caps[4].trim();
    let fields = parse_fields(fields_raw);

    // Default plural = snake_name + "s" (overridden by descriptor if available)
    let plural = format!("{}s", &snake_name);

    Ok(EntityMeta {
        pascal_name,
        snake_name,
        plural,
        indexed_fields,
        fields,
        routes: Vec::new(),
    })
}

/// Parse `"a", "b", "c"` into `vec!["a", "b", "c"]`
fn parse_string_list(raw: &str) -> Vec<String> {
    let re = Regex::new(r#""([^"]+)""#).unwrap();
    re.captures_iter(raw).map(|c| c[1].to_string()).collect()
}

/// Parse field lines like `name: String,` or `price: f64,`
fn parse_fields(raw: &str) -> Vec<FieldMeta> {
    let field_re = Regex::new(r"(\w+)\s*:\s*(.+?)\s*,").unwrap();
    field_re
        .captures_iter(raw)
        .map(|c| FieldMeta {
            name: c[1].to_string(),
            rust_type: c[2].trim().to_string(),
        })
        .collect()
}

// ── Descriptor parser ─────────────────────────────────────────────────

/// Parse a descriptor.rs to extract the plural form and REST routes.
pub fn parse_descriptor(path: &Path) -> Result<(String, Vec<RouteMeta>)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;

    parse_descriptor_content(&content)
}

fn parse_descriptor_content(content: &str) -> Result<(String, Vec<RouteMeta>)> {
    // Extract plural from: fn plural(&self) -> &str { "products" }
    let plural_re = Regex::new(r#"fn plural\(&self\)\s*->\s*&str\s*\{\s*"(\w+)""#).unwrap();
    let plural = plural_re
        .captures(content)
        .map(|c| c[1].to_string())
        .unwrap_or_default();

    // Extract routes from .route("/path", method_chain) patterns
    let mut routes = Vec::new();

    // Pattern: .route("/plural", get(list_xxx).post(create_xxx))
    // Pattern: .route("/plural/{id}", get(get_xxx).put(update_xxx).delete(delete_xxx))
    let route_re = Regex::new(r#"\.route\(\s*"(/[^"]+)"\s*,\s*([^)]+\)(?:\.[^)]+\))*)"#).unwrap();
    let method_re = Regex::new(r"(get|post|put|delete)\(").unwrap();

    for caps in route_re.captures_iter(content) {
        let path = caps[1].to_string();
        let methods_chain = &caps[2];

        // Extract each HTTP method from the chain: get(...), post(...), put(...), delete(...)
        for method_cap in method_re.captures_iter(methods_chain) {
            let method = method_cap[1].to_uppercase();
            routes.push(RouteMeta {
                method,
                path: path.clone(),
            });
        }
    }

    Ok((plural, routes))
}

// ── Links parser ──────────────────────────────────────────────────────

/// Lightweight links.yaml structures for deserialization.
/// We reuse the shape from add_link.rs but define our own minimal types
/// to avoid coupling codegen to the CLI command module.
#[derive(serde::Deserialize)]
struct LinksYaml {
    #[serde(default)]
    links: Vec<LinkEntry>,
}

#[derive(serde::Deserialize)]
struct LinkEntry {
    link_type: String,
    source_type: String,
    target_type: String,
    forward_route_name: String,
    reverse_route_name: String,
}

/// Parse config/links.yaml into LinkMeta entries.
pub fn parse_links_yaml(path: &Path) -> Result<Vec<LinkMeta>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;

    parse_links_yaml_content(&content)
}

fn parse_links_yaml_content(content: &str) -> Result<Vec<LinkMeta>> {
    let yaml: LinksYaml =
        serde_yaml::from_str(content).with_context(|| "Failed to parse links.yaml")?;

    Ok(yaml
        .links
        .into_iter()
        .map(|l| LinkMeta {
            link_type: l.link_type,
            source: l.source_type,
            target: l.target_type,
            forward_route: l.forward_route_name,
            reverse_route: l.reverse_route_name,
        })
        .collect())
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── parse_entity_model tests ──────────────────────────────────

    #[test]
    fn test_parse_entity_model_basic() {
        let content = r#"
use this::prelude::*;

impl_data_entity!(
    Product,
    "product",
    ["name"],
    {
        name: String,
        price: f64,
        description: Option<String>,
    }
);
"#;
        let entity = parse_entity_model_content(content, Path::new("test/model.rs")).unwrap();
        assert_eq!(entity.pascal_name, "Product");
        assert_eq!(entity.snake_name, "product");
        assert_eq!(entity.indexed_fields, vec!["name"]);
        assert_eq!(entity.fields.len(), 3);
        assert_eq!(entity.fields[0].name, "name");
        assert_eq!(entity.fields[0].rust_type, "String");
        assert_eq!(entity.fields[1].name, "price");
        assert_eq!(entity.fields[1].rust_type, "f64");
        assert_eq!(entity.fields[2].name, "description");
        assert_eq!(entity.fields[2].rust_type, "Option<String>");
    }

    #[test]
    fn test_parse_entity_model_validated() {
        let content = r#"
use this::prelude::*;

impl_data_entity_validated!(
    Order,
    "order",
    ["reference", "customer"],
    {
        reference: String,
        customer: String,
        total: f64,
    }
);
"#;
        let entity = parse_entity_model_content(content, Path::new("test/model.rs")).unwrap();
        assert_eq!(entity.pascal_name, "Order");
        assert_eq!(entity.snake_name, "order");
        assert_eq!(entity.indexed_fields, vec!["reference", "customer"]);
        assert_eq!(entity.fields.len(), 3);
    }

    #[test]
    fn test_parse_entity_model_no_macro() {
        let content = "use this::prelude::*;\n// no macro here\n";
        let result = parse_entity_model_content(content, Path::new("test/model.rs"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_entity_model_no_indexed_fields() {
        let content = r#"
impl_data_entity!(
    Tag,
    "tag",
    [],
    {
        label: String,
    }
);
"#;
        let entity = parse_entity_model_content(content, Path::new("test/model.rs")).unwrap();
        assert_eq!(entity.pascal_name, "Tag");
        assert!(entity.indexed_fields.is_empty());
        assert_eq!(entity.fields.len(), 1);
    }

    // ── parse_descriptor tests ────────────────────────────────────

    #[test]
    fn test_parse_descriptor_basic() {
        let content = r#"
impl EntityDescriptor for ProductDescriptor {
    fn entity_type(&self) -> &str {
        "product"
    }

    fn plural(&self) -> &str {
        "products"
    }

    fn build_routes(&self) -> Router {
        let state = ProductState {
            store: self.store.clone(),
            entity_creator: self.entity_creator.clone(),
        };
        Router::new()
            .route("/products", get(list_products).post(create_product))
            .route(
                "/products/{id}",
                get(get_product).put(update_product).delete(delete_product),
            )
            .with_state(state)
    }
}
"#;
        let (plural, routes) = parse_descriptor_content(content).unwrap();
        assert_eq!(plural, "products");
        assert_eq!(routes.len(), 5);
        assert_eq!(
            routes[0],
            RouteMeta {
                method: "GET".to_string(),
                path: "/products".to_string()
            }
        );
        assert_eq!(
            routes[1],
            RouteMeta {
                method: "POST".to_string(),
                path: "/products".to_string()
            }
        );
        assert_eq!(
            routes[2],
            RouteMeta {
                method: "GET".to_string(),
                path: "/products/{id}".to_string()
            }
        );
        assert_eq!(
            routes[3],
            RouteMeta {
                method: "PUT".to_string(),
                path: "/products/{id}".to_string()
            }
        );
        assert_eq!(
            routes[4],
            RouteMeta {
                method: "DELETE".to_string(),
                path: "/products/{id}".to_string()
            }
        );
    }

    #[test]
    fn test_parse_descriptor_no_plural() {
        let content = "fn build_routes(&self) -> Router { Router::new() }";
        let (plural, routes) = parse_descriptor_content(content).unwrap();
        assert_eq!(plural, "");
        assert!(routes.is_empty());
    }

    // ── parse_links_yaml tests ────────────────────────────────────

    #[test]
    fn test_parse_links_yaml_empty() {
        let content = "entities: []\nlinks: []\nvalidation_rules: {}\n";
        let links = parse_links_yaml_content(content).unwrap();
        assert!(links.is_empty());
    }

    #[test]
    fn test_parse_links_yaml_with_links() {
        let content = r#"
entities:
  - singular: order
    plural: orders
  - singular: invoice
    plural: invoices
links:
  - link_type: has_invoice
    source_type: order
    target_type: invoice
    forward_route_name: invoices
    reverse_route_name: order
    description: "Order -> Invoice relationship"
validation_rules:
  has_invoice:
    - source: order
      targets: [invoice]
"#;
        let links = parse_links_yaml_content(content).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, "has_invoice");
        assert_eq!(links[0].source, "order");
        assert_eq!(links[0].target, "invoice");
        assert_eq!(links[0].forward_route, "invoices");
        assert_eq!(links[0].reverse_route, "order");
    }

    #[test]
    fn test_parse_links_yaml_multiple() {
        let content = r#"
links:
  - link_type: has_product
    source_type: category
    target_type: product
    forward_route_name: products
    reverse_route_name: category
  - link_type: has_review
    source_type: product
    target_type: review
    forward_route_name: reviews
    reverse_route_name: product
"#;
        let links = parse_links_yaml_content(content).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].link_type, "has_product");
        assert_eq!(links[1].link_type, "has_review");
    }

    // ── Full introspection integration test ───────────────────────

    #[test]
    fn test_introspect_full_project() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create entity directory structure
        let product_dir = root.join("src/entities/product");
        std::fs::create_dir_all(&product_dir).unwrap();

        // model.rs
        std::fs::write(
            product_dir.join("model.rs"),
            r#"
use this::prelude::*;

impl_data_entity!(
    Product,
    "product",
    ["name", "sku"],
    {
        name: String,
        sku: String,
        price: f64,
    }
);
"#,
        )
        .unwrap();

        // descriptor.rs
        std::fs::write(
            product_dir.join("descriptor.rs"),
            r#"
impl EntityDescriptor for ProductDescriptor {
    fn entity_type(&self) -> &str {
        "product"
    }
    fn plural(&self) -> &str {
        "products"
    }
    fn build_routes(&self) -> Router {
        Router::new()
            .route("/products", get(list_products).post(create_product))
            .route("/products/{id}", get(get_product).put(update_product).delete(delete_product))
            .with_state(state)
    }
}
"#,
        )
        .unwrap();

        // config/links.yaml
        let config_dir = root.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("links.yaml"),
            r#"
entities:
  - singular: product
    plural: products
links:
  - link_type: has_review
    source_type: product
    target_type: review
    forward_route_name: reviews
    reverse_route_name: product
validation_rules: {}
"#,
        )
        .unwrap();

        let result = introspect(root).unwrap();

        // Check entities
        assert_eq!(result.entities.len(), 1);
        let product = &result.entities[0];
        assert_eq!(product.pascal_name, "Product");
        assert_eq!(product.snake_name, "product");
        assert_eq!(product.plural, "products");
        assert_eq!(product.indexed_fields, vec!["name", "sku"]);
        assert_eq!(product.fields.len(), 3);
        assert_eq!(product.routes.len(), 5);

        // Check links
        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].link_type, "has_review");
    }

    #[test]
    fn test_introspect_empty_project() {
        let tmp = TempDir::new().unwrap();
        let result = introspect(tmp.path()).unwrap();
        assert!(result.entities.is_empty());
        assert!(result.links.is_empty());
    }

    #[test]
    fn test_introspect_no_descriptor() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        let entity_dir = root.join("src/entities/tag");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("model.rs"),
            r#"
impl_data_entity!(
    Tag,
    "tag",
    ["label"],
    {
        label: String,
    }
);
"#,
        )
        .unwrap();

        let result = introspect(root).unwrap();
        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].pascal_name, "Tag");
        assert_eq!(result.entities[0].plural, "tags"); // default plural
        assert!(result.entities[0].routes.is_empty()); // no descriptor
    }

    #[test]
    fn test_introspect_multiple_entities_sorted() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();

        // Create two entities — order should come before product alphabetically
        for (name, pascal) in &[("order", "Order"), ("product", "Product")] {
            let dir = root.join(format!("src/entities/{}", name));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("model.rs"),
                format!(
                    "impl_data_entity!({}, \"{}\", [\"name\"], {{ name: String, }});",
                    pascal, name
                ),
            )
            .unwrap();
        }

        let result = introspect(root).unwrap();
        assert_eq!(result.entities.len(), 2);
        assert_eq!(result.entities[0].snake_name, "order");
        assert_eq!(result.entities[1].snake_name, "product");
    }

    // ── parse_string_list tests ───────────────────────────────────

    #[test]
    fn test_parse_string_list() {
        assert_eq!(parse_string_list(r#""a", "b", "c""#), vec!["a", "b", "c"]);
        assert_eq!(parse_string_list(r#""single""#), vec!["single"]);
        assert!(parse_string_list("").is_empty());
    }

    // ── parse_fields tests ────────────────────────────────────────

    #[test]
    fn test_parse_fields() {
        let fields = parse_fields("  name: String,\n  price: f64,\n  desc: Option<String>,\n");
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].name, "name");
        assert_eq!(fields[0].rust_type, "String");
        assert_eq!(fields[2].rust_type, "Option<String>");
    }
}
