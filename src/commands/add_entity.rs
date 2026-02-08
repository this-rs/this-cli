use anyhow::{Context, Result, bail};

use super::AddEntityArgs;
use crate::templates::TemplateEngine;
use crate::utils::{naming, output, project};

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

    output::print_step(&format!(
        "Adding entity '{}' to project...",
        &entity_name
    ));

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
        let rendered = engine.render(tpl, &context)
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

    output::print_success(&format!("Entity '{}' created!", &entity_name));
    output::print_next_steps(&[
        "Don't forget to:",
        &format!(
            "  1. Register the entity in src/module.rs:"
        ),
        &format!(
            "     - Add to entity_types(): \"{}\"",
            &entity_name
        ),
        &format!(
            "     - Add to register_entities(): registry.register(Box::new({}Descriptor::new_with_creator(...)))",
            &entity_pascal
        ),
        &format!(
            "     - Add to get_entity_fetcher/get_entity_creator: \"{}\" => ...",
            &entity_name
        ),
        &format!(
            "  2. Add to config/links.yaml entities section if needed"
        ),
    ]);

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
        assert!(result.unwrap_err().to_string().contains("Invalid field format"));
    }

    #[test]
    fn test_parse_fields_unsupported_type() {
        let result = parse_fields("x:HashMap");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unsupported field type"));
    }
}
