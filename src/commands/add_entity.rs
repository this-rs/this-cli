use std::path::Path;

use anyhow::{Context, Result, bail};

use super::AddEntityArgs;
use crate::templates::TemplateEngine;
use crate::utils::{markers, naming, output, project};

/// Parsed field definition
#[derive(Debug, Clone, serde::Serialize)]
pub struct Field {
    pub name: String,
    pub rust_type: String,
    pub is_optional: bool,
}

/// Parse a fields string like "sku:String,price:f64,description:Option<String>"
pub fn parse_fields(input: &str) -> Result<Vec<Field>> {
    let mut fields = Vec::new();

    for pair in input.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }

        let parts: Vec<&str> = pair.splitn(2, ':').collect();
        if parts.len() != 2 {
            bail!(
                "Invalid field format: '{}'. Expected 'name:Type' (e.g. 'sku:String')",
                pair
            );
        }

        let name = parts[0].trim().to_string();
        let rust_type = parts[1].trim().to_string();

        // Validate supported types
        let base_type = rust_type
            .strip_prefix("Option<")
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(&rust_type);

        let supported_types = [
            "String", "f64", "f32", "i32", "i64", "u32", "u64", "bool", "Uuid",
        ];

        if !supported_types.contains(&base_type) {
            bail!(
                "Unsupported field type: '{}'. Supported types: {}",
                base_type,
                supported_types.join(", ")
            );
        }

        let is_optional = rust_type.starts_with("Option<");

        fields.push(Field {
            name,
            rust_type,
            is_optional,
        });
    }

    Ok(fields)
}

pub fn run(args: AddEntityArgs) -> Result<()> {
    let project_root = project::detect_project_root()?;
    let entity_name = naming::to_snake_case(&args.name);
    let entity_pascal = naming::to_pascal_case(&args.name);
    let entity_plural = naming::pluralize(&entity_name);

    let entity_dir = project_root.join("src/entities").join(&entity_name);
    if entity_dir.exists() {
        bail!(
            "Entity '{}' already exists at {}",
            &entity_name,
            entity_dir.display()
        );
    }

    // Parse fields
    let fields = match &args.fields {
        Some(f) => parse_fields(f)?,
        None => vec![],
    };

    // Parse indexed fields
    let indexed_fields: Vec<String> = args
        .indexed
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    output::print_step(&format!("Adding entity '{}' to project...", &entity_name));

    // Create entity directory
    std::fs::create_dir_all(&entity_dir)
        .with_context(|| format!("Failed to create: {}", entity_dir.display()))?;

    // Prepare template context
    let engine = TemplateEngine::new()?;
    let mut context = tera::Context::new();
    context.insert("entity_name", &entity_name);
    context.insert("entity_pascal", &entity_pascal);
    context.insert("entity_plural", &entity_plural);
    context.insert("fields", &fields);
    context.insert("indexed_fields", &indexed_fields);
    context.insert("validated", &args.validated);

    // Generate entity files
    let template_name = if args.validated {
        "entity/model_validated.rs"
    } else {
        "entity/model.rs"
    };

    let entity_files: &[(&str, &str)] = &[
        (template_name, "model.rs"),
        ("entity/store.rs", "store.rs"),
        ("entity/handlers.rs", "handlers.rs"),
        ("entity/descriptor.rs", "descriptor.rs"),
        ("entity/mod.rs", "mod.rs"),
    ];

    for (tpl, filename) in entity_files {
        let rendered = engine
            .render(tpl, &context)
            .with_context(|| format!("Failed to render template: {}", tpl))?;
        let file_path = entity_dir.join(filename);
        std::fs::write(&file_path, &rendered)
            .with_context(|| format!("Failed to write: {}", file_path.display()))?;
        output::print_file_created(&format!("src/entities/{}/{}", &entity_name, filename));
    }

    // Update src/entities/mod.rs
    let entities_mod_path = project_root.join("src/entities/mod.rs");
    let mod_declaration = format!("pub mod {};", &entity_name);

    if entities_mod_path.exists() {
        let content = std::fs::read_to_string(&entities_mod_path)?;
        if !content.contains(&mod_declaration) {
            let new_content = if content.trim().is_empty() {
                format!("{}\n", &mod_declaration)
            } else {
                format!("{}\n{}\n", content.trim_end(), &mod_declaration)
            };
            std::fs::write(&entities_mod_path, new_content)?;
            output::print_info(&format!(
                "Updated src/entities/mod.rs (added pub mod {})",
                &entity_name
            ));
        }
    } else {
        std::fs::write(&entities_mod_path, format!("{}\n", &mod_declaration))?;
        output::print_info("Created src/entities/mod.rs");
    }

    // Update src/stores.rs (marker-based insertion)
    update_stores_rs(&project_root, &entity_name, &entity_pascal, &entity_plural)?;

    output::print_success(&format!("Entity '{}' created!", &entity_name));
    output::print_next_steps(&[
        "Don't forget to:",
        "  1. Add to config/links.yaml entities section if needed",
    ]);

    Ok(())
}

/// Update src/stores.rs to add the new entity's store fields and initialization.
///
/// Uses marker-based insertion for idempotent updates:
/// - `[this:store_fields]` — struct fields
/// - `[this:store_init_vars]` — variable initialization
/// - `[this:store_init_fields]` — struct init fields
fn update_stores_rs(
    project_root: &Path,
    entity_name: &str,
    entity_pascal: &str,
    entity_plural: &str,
) -> Result<()> {
    let stores_path = project_root.join("src/stores.rs");
    if !stores_path.exists() {
        output::print_warn("src/stores.rs not found — skipping stores registration");
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&stores_path).with_context(|| "Failed to read src/stores.rs")?;

    // Check markers exist
    if !content.contains("[this:store_fields]") {
        output::print_warn(
            "src/stores.rs has no [this:store_fields] marker — skipping stores registration.\n\
             Hint: regenerate your project with `this init` to get marker-based templates.",
        );
        return Ok(());
    }

    // Idempotence check
    let field_needle = format!("{}_store:", entity_plural);
    if markers::has_line_after_marker(&content, "[this:store_fields]", &field_needle) {
        output::print_info(&format!(
            "stores.rs already contains {} — skipping",
            field_needle
        ));
        return Ok(());
    }

    // 1. Add store fields after [this:store_fields]
    let store_field = format!(
        "pub {plural}_store: Arc<dyn {pascal}Store>,",
        plural = entity_plural,
        pascal = entity_pascal
    );
    let entity_field = format!(
        "pub {plural}_entity: Arc<dyn EntityStore>,",
        plural = entity_plural
    );
    let mut updated = markers::insert_after_marker(&content, "[this:store_fields]", &store_field)?;
    updated = markers::insert_after_marker(&updated, &store_field, &entity_field)?;

    // 2. Add init var after [this:store_init_vars]
    let init_var = format!(
        "let {plural} = Arc::new(InMemory{pascal}Store::default());",
        plural = entity_plural,
        pascal = entity_pascal
    );
    updated = markers::insert_after_marker(&updated, "[this:store_init_vars]", &init_var)?;

    // 3. Add init fields after [this:store_init_fields]
    let init_store_field = format!("{plural}_store: {plural}.clone(),", plural = entity_plural);
    let init_entity_field = format!("{plural}_entity: {plural},", plural = entity_plural);
    updated =
        markers::insert_after_marker(&updated, "[this:store_init_fields]", &init_store_field)?;
    updated = markers::insert_after_marker(&updated, &init_store_field, &init_entity_field)?;

    // 4. Add imports
    let import_line = format!(
        "use crate::entities::{name}::{{InMemory{pascal}Store, {pascal}Store}};",
        name = entity_name,
        pascal = entity_pascal
    );
    updated = markers::add_import(&updated, &import_line);

    std::fs::write(&stores_path, updated).with_context(|| "Failed to write src/stores.rs")?;

    output::print_info(&format!(
        "Updated src/stores.rs (added {} store)",
        entity_name
    ));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fields_valid() {
        let fields = parse_fields("sku:String,price:f64").unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "sku");
        assert_eq!(fields[0].rust_type, "String");
        assert!(!fields[0].is_optional);
        assert_eq!(fields[1].name, "price");
        assert_eq!(fields[1].rust_type, "f64");
        assert!(!fields[1].is_optional);
    }

    #[test]
    fn test_parse_fields_optional() {
        let fields = parse_fields("description:Option<String>").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "description");
        assert_eq!(fields[0].rust_type, "Option<String>");
        assert!(fields[0].is_optional);
    }

    #[test]
    fn test_parse_fields_all_types() {
        let input = "a:String,b:f64,c:f32,d:i32,e:i64,f:u32,g:u64,h:bool,i:Uuid";
        let fields = parse_fields(input).unwrap();
        assert_eq!(fields.len(), 9);
    }

    #[test]
    fn test_parse_fields_with_spaces() {
        let fields = parse_fields("  sku : String , price : f64  ").unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "sku");
        assert_eq!(fields[0].rust_type, "String");
    }

    #[test]
    fn test_parse_fields_empty() {
        let fields = parse_fields("").unwrap();
        assert_eq!(fields.len(), 0);
    }

    #[test]
    fn test_parse_fields_invalid_format() {
        let result = parse_fields("invalid");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid field format")
        );
    }

    #[test]
    fn test_parse_fields_unsupported_type() {
        let result = parse_fields("x:HashMap");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unsupported field type")
        );
    }
}
