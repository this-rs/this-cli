use std::path::Path;

use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::AddEntityArgs;
use crate::templates::TemplateEngine;
use crate::utils::file_writer::FileWriter;
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

pub fn run(args: AddEntityArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
    run_in(args, writer, &cwd)
}

/// Run the add entity command with an explicit starting directory.
/// This avoids relying on the process-global CWD, making it safe for parallel tests.
pub(crate) fn run_in(
    args: AddEntityArgs,
    writer: &dyn FileWriter,
    cwd: &std::path::Path,
) -> Result<()> {
    let project_root = project::detect_project_root_from(cwd)?;
    let entity_name = naming::to_snake_case(&args.name);
    let entity_pascal = naming::to_pascal_case(&args.name);
    let entity_plural = naming::pluralize(&entity_name);

    let entity_dir = project_root.join("src/entities").join(&entity_name);
    if entity_dir.exists() && !writer.is_dry_run() {
        bail!(
            "Entity '{}' already exists at {}",
            &entity_name,
            entity_dir.display()
        );
    }

    if writer.is_dry_run() {
        println!("🔍 {}", "Dry run — no files will be written".cyan().bold());
        println!();
    }

    // Parse fields and filter out reserved fields (already provided by impl_data_entity! macro)
    let reserved_fields = [
        "id",
        "entity_type",
        "name",
        "status",
        "created_at",
        "updated_at",
        "deleted_at",
    ];

    let fields = match &args.fields {
        Some(f) => {
            let parsed = parse_fields(f)?;
            let (reserved, custom): (Vec<_>, Vec<_>) = parsed
                .into_iter()
                .partition(|f| reserved_fields.contains(&f.name.as_str()));
            for field in &reserved {
                output::print_warn(&format!(
                    "Field '{}' is built-in (provided by impl_data_entity! macro) — skipping",
                    field.name
                ));
            }
            custom
        }
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
    writer.create_dir_all(&entity_dir)?;

    // Prepare template context
    let engine = TemplateEngine::new()?;
    let mut context = tera::Context::new();
    context.insert("entity_name", &entity_name);
    context.insert("entity_pascal", &entity_pascal);
    context.insert("entity_plural", &entity_plural);
    context.insert("fields", &fields);
    context.insert("indexed_fields", &indexed_fields);
    context.insert("validated", &args.validated);
    context.insert("backend", &args.backend);

    // Generate entity files
    let template_name = if args.validated {
        "entity/model_validated.rs"
    } else {
        "entity/model.rs"
    };

    let store_template = match args.backend.as_str() {
        "postgres" => "entity/postgres_store.rs",
        "mongodb" => "entity/mongodb_store.rs",
        "neo4j" => "entity/neo4j_store.rs",
        "scylladb" => "entity/scylladb_store.rs",
        "mysql" => "entity/mysql_store.rs",
        "lmdb" => "entity/lmdb_store.rs",
        _ => "entity/store.rs",
    };

    let entity_files: &[(&str, &str)] = &[
        (template_name, "model.rs"),
        (store_template, "store.rs"),
        ("entity/handlers.rs", "handlers.rs"),
        ("entity/descriptor.rs", "descriptor.rs"),
        ("entity/mod.rs", "mod.rs"),
    ];

    for (tpl, filename) in entity_files {
        let rendered = engine
            .render(tpl, &context)
            .with_context(|| format!("Failed to render template: {}", tpl))?;
        let file_path = entity_dir.join(filename);
        writer.write_file(&file_path, &rendered)?;
        if !writer.is_dry_run() {
            output::print_file_created(&format!("src/entities/{}/{}", &entity_name, filename));
        }
    }

    // Generate SQL migration for SQL backends (postgres, mysql)
    if args.backend == "postgres" || args.backend == "mysql" {
        generate_sql_migration(
            &project_root,
            &entity_name,
            &args.backend,
            &engine,
            &context,
            writer,
        )?;
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
            writer.update_file(&entities_mod_path, &content, &new_content)?;
            if !writer.is_dry_run() {
                output::print_info(&format!(
                    "Updated src/entities/mod.rs (added pub mod {})",
                    &entity_name
                ));
            }
        }
    } else {
        writer.write_file(&entities_mod_path, &format!("{}\n", &mod_declaration))?;
        if !writer.is_dry_run() {
            output::print_info("Created src/entities/mod.rs");
        }
    }

    // Update src/stores.rs (marker-based insertion)
    update_stores_rs(
        &project_root,
        &entity_name,
        &entity_pascal,
        &entity_plural,
        &args.backend,
        writer,
    )?;

    // Update src/module.rs (marker-based insertion)
    update_module_rs(
        &project_root,
        &entity_name,
        &entity_pascal,
        &entity_plural,
        writer,
    )?;

    // Update config/links.yaml (add entity config)
    update_links_yaml(&project_root, &entity_name, &entity_plural, writer)?;

    if !writer.is_dry_run() {
        output::print_success(&format!("Entity '{}' created!", &entity_name));
        println!();
        match args.backend.as_str() {
            "postgres" => {
                println!("  Backend: {}", "PostgreSQL (sqlx)".bold());
                println!();
                println!("  Next steps:");
                println!(
                    "    1. Add {} to your Cargo.toml",
                    "this = { features = [\"postgres\"] }".bold()
                );
                println!("    2. Run migrations: {}", "sqlx migrate run".bold());
                println!(
                    "    3. Update main.rs to use {}",
                    "Stores::new_postgres(pool)".bold()
                );
                println!("    4. Run: {}", "cargo run --features postgres".bold());
            }
            "mongodb" => {
                println!("  Backend: {}", "MongoDB".bold());
                println!();
                println!("  Next steps:");
                println!(
                    "    1. Add {} to your Cargo.toml",
                    "this = { features = [\"mongodb\"] }".bold()
                );
                println!(
                    "    2. Start MongoDB: {}",
                    "docker run -d -p 27017:27017 mongo".bold()
                );
                println!(
                    "    3. Update main.rs to use {}",
                    "Stores::new_mongodb(db)".bold()
                );
                println!("    4. Run: {}", "cargo run --features mongodb".bold());
            }
            "neo4j" => {
                println!("  Backend: {}", "Neo4j".bold());
                println!();
                println!("  Next steps:");
                println!(
                    "    1. Add {} to your Cargo.toml",
                    "this = { features = [\"neo4j\"] }".bold()
                );
                println!(
                    "    2. Start Neo4j: {}",
                    "docker run -d -p 7687:7687 -e NEO4J_AUTH=none neo4j".bold()
                );
                println!(
                    "    3. Update main.rs to use {}",
                    "Stores::new_neo4j(graph)".bold()
                );
                println!("    4. Run: {}", "cargo run --features neo4j".bold());
            }
            "scylladb" => {
                println!("  Backend: {}", "ScyllaDB".bold());
                println!();
                println!("  Next steps:");
                println!(
                    "    1. Add {} to your Cargo.toml",
                    "this = { features = [\"scylladb\"] }".bold()
                );
                println!(
                    "    2. Start ScyllaDB: {}",
                    "docker run -d -p 9042:9042 scylladb/scylla".bold()
                );
                println!(
                    "    3. Update main.rs to use {}",
                    "Stores::new_scylladb(session, keyspace)".bold()
                );
                println!("    4. Run: {}", "cargo run --features scylladb".bold());
            }
            "mysql" => {
                println!("  Backend: {}", "MySQL (sqlx)".bold());
                println!();
                println!("  Next steps:");
                println!(
                    "    1. Add {} to your Cargo.toml",
                    "this = { features = [\"mysql\"] }".bold()
                );
                println!("    2. Run migrations: {}", "sqlx migrate run".bold());
                println!(
                    "    3. Update main.rs to use {}",
                    "Stores::new_mysql(pool)".bold()
                );
                println!("    4. Run: {}", "cargo run --features mysql".bold());
            }
            "lmdb" => {
                println!("  Backend: {}", "LMDB (embedded)".bold());
                println!();
                println!("  Next steps:");
                println!(
                    "    1. Add {} to your Cargo.toml",
                    "this = { features = [\"lmdb\"] }".bold()
                );
                println!(
                    "    2. Update main.rs to use {}",
                    "Stores::new_lmdb(env)".bold()
                );
                println!("    3. Run: {}", "cargo run --features lmdb".bold());
            }
            _ => {
                println!("  Your project is ready to run: {}", "cargo run".bold());
            }
        }
        println!();
    }

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
    backend: &str,
    writer: &dyn FileWriter,
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

    // 1. Add store fields after [this:store_fields] (same for any backend — trait objects)
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

    match backend {
        "postgres" => {
            let import = format!(
                "use crate::entities::{name}::{{{pascal}Store, Postgres{pascal}Store}};",
                name = entity_name,
                pascal = entity_pascal
            );
            updated = markers::add_import(&updated, &import);
            updated = ensure_backend_constructor(
                &updated,
                entity_name,
                entity_pascal,
                entity_plural,
                "pg",
                "postgres",
                "Postgres",
                "#[cfg(feature = \"postgres\")]",
                "pool: sqlx::PgPool",
                "Postgres{pascal}Store::new(pool.clone())",
            )?;
        }
        "mongodb" => {
            let import = format!(
                "use crate::entities::{name}::{{{pascal}Store, Mongo{pascal}Store}};",
                name = entity_name,
                pascal = entity_pascal
            );
            updated = markers::add_import(&updated, &import);
            updated = ensure_backend_constructor(
                &updated,
                entity_name,
                entity_pascal,
                entity_plural,
                "mongo",
                "mongodb",
                "Mongo",
                "#[cfg(feature = \"mongodb\")]",
                "db: mongodb::Database",
                "Mongo{pascal}Store::new(db.clone())",
            )?;
        }
        "neo4j" => {
            let import = format!(
                "use crate::entities::{name}::{{{pascal}Store, Neo4j{pascal}Store}};",
                name = entity_name,
                pascal = entity_pascal
            );
            updated = markers::add_import(&updated, &import);
            updated = ensure_backend_constructor(
                &updated,
                entity_name,
                entity_pascal,
                entity_plural,
                "neo4j",
                "neo4j",
                "Neo4j",
                "#[cfg(feature = \"neo4j\")]",
                "graph: std::sync::Arc<neo4rs::Graph>",
                "Neo4j{pascal}Store::new(graph.clone())",
            )?;
        }
        "scylladb" => {
            let import = format!(
                "use crate::entities::{name}::{{{pascal}Store, Scylla{pascal}Store}};",
                name = entity_name,
                pascal = entity_pascal
            );
            updated = markers::add_import(&updated, &import);
            updated = ensure_backend_constructor(
                &updated,
                entity_name,
                entity_pascal,
                entity_plural,
                "scylla",
                "scylladb",
                "Scylla",
                "#[cfg(feature = \"scylladb\")]",
                "session: std::sync::Arc<scylla::client::session::Session>, keyspace: &str",
                "Scylla{pascal}Store::new(session.clone(), keyspace)",
            )?;
        }
        "mysql" => {
            let import = format!(
                "use crate::entities::{name}::{{{pascal}Store, Mysql{pascal}Store}};",
                name = entity_name,
                pascal = entity_pascal
            );
            updated = markers::add_import(&updated, &import);
            updated = ensure_backend_constructor(
                &updated,
                entity_name,
                entity_pascal,
                entity_plural,
                "mysql",
                "mysql",
                "Mysql",
                "#[cfg(feature = \"mysql\")]",
                "pool: sqlx::MySqlPool",
                "Mysql{pascal}Store::new(pool.clone())",
            )?;
        }
        "lmdb" => {
            let import = format!(
                "use crate::entities::{name}::{{{pascal}Store, Lmdb{pascal}Store}};",
                name = entity_name,
                pascal = entity_pascal
            );
            updated = markers::add_import(&updated, &import);
            updated = ensure_backend_constructor(
                &updated,
                entity_name,
                entity_pascal,
                entity_plural,
                "lmdb",
                "lmdb",
                "Lmdb",
                "#[cfg(feature = \"lmdb\")]",
                "env: std::sync::Arc<heed::Env>",
                "Lmdb{pascal}Store::new(env.clone())",
            )?;
        }
        _ => {
            // In-memory backend: add init in new_in_memory() constructor
            let inmemory_init_var = format!(
                "let {plural} = Arc::new(InMemory{pascal}Store::default());",
                plural = entity_plural,
                pascal = entity_pascal
            );
            updated = markers::insert_after_marker(
                &updated,
                "[this:store_init_vars]",
                &inmemory_init_var,
            )?;

            let init_store_field =
                format!("{plural}_store: {plural}.clone(),", plural = entity_plural);
            let init_entity_field = format!("{plural}_entity: {plural},", plural = entity_plural);
            updated = markers::insert_after_marker(
                &updated,
                "[this:store_init_fields]",
                &init_store_field,
            )?;
            updated =
                markers::insert_after_marker(&updated, &init_store_field, &init_entity_field)?;

            let inmemory_import = format!(
                "use crate::entities::{name}::{{InMemory{pascal}Store, {pascal}Store}};",
                name = entity_name,
                pascal = entity_pascal
            );
            updated = markers::add_import(&updated, &inmemory_import);
        }
    }

    writer.update_file(&stores_path, &content, &updated)?;

    if !writer.is_dry_run() {
        output::print_info(&format!(
            "Updated src/stores.rs (added {} store, backend: {})",
            entity_name, backend
        ));
    }

    Ok(())
}

/// Ensure stores.rs has a backend-specific constructor with markers,
/// and add the entity's init inside it.
///
/// - `marker_prefix`: short name for markers, e.g. "pg", "mongo", "neo4j"
/// - `backend_name`: feature name, e.g. "postgres", "mongodb", "neo4j"
/// - `store_prefix`: store type prefix, e.g. "Postgres", "Mongo", "Neo4j"
/// - `cfg_attr`: the `#[cfg(...)]` attribute, e.g. `#[cfg(feature = "postgres")]`
/// - `constructor_params`: parameter signature, e.g. "pool: sqlx::PgPool"
/// - `store_new_expr`: expression for creating the store, with `{pascal}` placeholder
#[allow(clippy::too_many_arguments)]
fn ensure_backend_constructor(
    content: &str,
    _entity_name: &str,
    entity_pascal: &str,
    entity_plural: &str,
    marker_prefix: &str,
    backend_name: &str,
    store_prefix: &str,
    cfg_attr: &str,
    constructor_params: &str,
    store_new_expr: &str,
) -> Result<String> {
    let mut updated = content.to_string();

    let vars_marker = format!("[this:store_{}_init_vars]", marker_prefix);
    let fields_marker = format!("[this:store_{}_init_fields]", marker_prefix);

    // If the markers don't exist yet, add the constructor
    if !updated.contains(&vars_marker) {
        let last_closing = updated.rfind("\n}").ok_or_else(|| {
            anyhow::anyhow!("Cannot find closing brace of impl block in stores.rs")
        })?;

        let constructor = format!(
            r#"
    /// Create stores backed by {backend_display}.
    ///
    /// Requires the `{feature}` feature.
    {cfg}
    pub fn new_{fn_name}({params}) -> Self {{
        // {vars_mk}

        Self {{
            // {fields_mk}
        }}
    }}
"#,
            backend_display = store_prefix,
            feature = backend_name,
            cfg = cfg_attr,
            fn_name = backend_name,
            params = constructor_params,
            vars_mk = vars_marker,
            fields_mk = fields_marker,
        );
        updated.insert_str(last_closing, &constructor);
    }

    // Add init var
    let init_var = format!(
        "let {plural} = Arc::new({new_expr});",
        plural = entity_plural,
        new_expr = store_new_expr.replace("{pascal}", entity_pascal),
    );
    updated = markers::insert_after_marker(&updated, &vars_marker, &init_var)?;

    // Add init fields
    let init_store_field = format!("{plural}_store: {plural}.clone(),", plural = entity_plural);
    let init_entity_field = format!("{plural}_entity: {plural},", plural = entity_plural);
    updated = markers::insert_after_marker(&updated, &fields_marker, &init_store_field)?;
    updated = markers::insert_after_marker(&updated, &init_store_field, &init_entity_field)?;

    Ok(updated)
}

/// Generate a SQL migration file for a SQL-backed entity (postgres or mysql).
fn generate_sql_migration(
    project_root: &Path,
    entity_name: &str,
    backend: &str,
    engine: &TemplateEngine,
    context: &tera::Context,
    writer: &dyn FileWriter,
) -> Result<()> {
    let migrations_dir = project_root.join("migrations");
    if !migrations_dir.exists() {
        writer.create_dir_all(&migrations_dir)?;
    }

    // Find the next migration number
    let next_num = if migrations_dir.exists() {
        let mut max_num = 0u32;
        if let Ok(entries) = std::fs::read_dir(&migrations_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(num_str) = name.split('_').next()
                    && let Ok(num) = num_str.parse::<u32>()
                {
                    max_num = max_num.max(num);
                }
            }
        }
        max_num + 1
    } else {
        1
    };

    let migration_filename = format!("{:03}_{}_index.up.sql", next_num, entity_name);
    let migration_path = migrations_dir.join(&migration_filename);

    let rendered = engine
        .render("entity/migration.sql", context)
        .with_context(|| "Failed to render migration template")?;

    writer.write_file(&migration_path, &rendered)?;

    if !writer.is_dry_run() {
        output::print_file_created(&format!("migrations/{} ({})", migration_filename, backend));
    }

    Ok(())
}

/// Update src/module.rs to register the new entity in all 4 marker sections.
///
/// Uses marker-based insertion for idempotent updates:
/// - `[this:entity_types]` — entity type string in vec![]
/// - `[this:register_entities]` — descriptor registration
/// - `[this:entity_fetcher]` — match arm for get_entity_fetcher
/// - `[this:entity_creator]` — match arm for get_entity_creator
fn update_module_rs(
    project_root: &Path,
    entity_name: &str,
    entity_pascal: &str,
    entity_plural: &str,
    writer: &dyn FileWriter,
) -> Result<()> {
    let module_path = project_root.join("src/module.rs");
    if !module_path.exists() {
        output::print_warn("src/module.rs not found — skipping module registration");
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&module_path).with_context(|| "Failed to read src/module.rs")?;

    // Check markers exist
    if !content.contains("[this:entity_types]") {
        output::print_warn(
            "src/module.rs has no [this:entity_types] marker — skipping module registration.\n\
             Hint: regenerate your project with `this init` to get marker-based templates.",
        );
        return Ok(());
    }

    // Idempotence check
    let type_needle = format!("\"{}\"", entity_name);
    if markers::has_line_after_marker(&content, "[this:entity_types]", &type_needle) {
        output::print_info(&format!(
            "module.rs already contains \"{}\" — skipping",
            entity_name
        ));
        return Ok(());
    }

    // 1. Add entity type after [this:entity_types]
    let entity_type_line = format!("\"{}\",", entity_name);
    let mut updated =
        markers::insert_after_marker(&content, "[this:entity_types]", &entity_type_line)?;

    // 2. Add descriptor registration after [this:register_entities]
    // Change _registry to registry since it's now used
    updated = updated.replace(
        "_registry: &mut EntityRegistry",
        "registry: &mut EntityRegistry",
    );
    let register_line = format!(
        "registry.register(Box::new({pascal}Descriptor::new_with_creator(self.stores.{plural}_store.clone(), self.stores.{plural}_entity.clone())));",
        pascal = entity_pascal,
        plural = entity_plural
    );
    updated = markers::insert_after_marker(&updated, "[this:register_entities]", &register_line)?;

    // 3. Add match arm for entity_fetcher after [this:entity_fetcher]
    // Change _entity_type to entity_type since it's now used
    updated = updated.replace(
        "fn get_entity_fetcher(&self, _entity_type: &str)",
        "fn get_entity_fetcher(&self, entity_type: &str)",
    );
    updated = updated.replace("match _entity_type {", "match entity_type {");
    let fetcher_line = format!(
        "\"{name}\" => Some(self.stores.{plural}_entity.clone()),",
        name = entity_name,
        plural = entity_plural
    );
    updated = markers::insert_after_marker(&updated, "[this:entity_fetcher]", &fetcher_line)?;

    // 4. Add match arm for entity_creator after [this:entity_creator]
    // Change _entity_type to entity_type since it's now used
    updated = updated.replace(
        "fn get_entity_creator(&self, _entity_type: &str)",
        "fn get_entity_creator(&self, entity_type: &str)",
    );
    let creator_line = format!(
        "\"{name}\" => Some(self.stores.{plural}_entity.clone()),",
        name = entity_name,
        plural = entity_plural
    );
    updated = markers::insert_after_marker(&updated, "[this:entity_creator]", &creator_line)?;

    // 5. Add imports
    let descriptor_import = format!(
        "use crate::entities::{name}::descriptor::{pascal}Descriptor;",
        name = entity_name,
        pascal = entity_pascal
    );
    updated = markers::add_import(&updated, &descriptor_import);

    writer.update_file(&module_path, &content, &updated)?;

    if !writer.is_dry_run() {
        output::print_info(&format!(
            "Updated src/module.rs (registered {} entity)",
            entity_name
        ));
    }

    Ok(())
}

/// Update config/links.yaml to add the entity config if not already present.
fn update_links_yaml(
    project_root: &Path,
    entity_name: &str,
    entity_plural: &str,
    writer: &dyn FileWriter,
) -> Result<()> {
    let links_path = project_root.join("config/links.yaml");
    if !links_path.exists() {
        output::print_warn("config/links.yaml not found — skipping entity config");
        return Ok(());
    }

    let yaml_content =
        std::fs::read_to_string(&links_path).with_context(|| "Failed to read config/links.yaml")?;
    let mut config: super::add_link::LinksConfig =
        serde_yaml::from_str(&yaml_content).with_context(|| "Failed to parse links.yaml")?;

    // Idempotence check
    if config.entities.iter().any(|e| e.singular == entity_name) {
        return Ok(());
    }

    config.entities.push(super::add_link::EntityConfig {
        singular: entity_name.to_string(),
        plural: entity_plural.to_string(),
        auth: super::add_link::default_entity_auth(),
    });

    let new_yaml =
        serde_yaml::to_string(&config).with_context(|| "Failed to serialize links.yaml")?;
    writer.update_file(&links_path, &yaml_content, &new_yaml)?;

    if !writer.is_dry_run() {
        output::print_info(&format!(
            "Updated config/links.yaml (added {} entity config)",
            entity_name
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use tempfile::TempDir;

    /// Create a minimal project scaffold suitable for `add entity` testing.
    ///
    /// The scaffold includes:
    /// - Cargo.toml with `this` dependency
    /// - src/entities/ directory (empty)
    /// - src/stores.rs with all markers
    /// - src/module.rs with all markers
    /// - config/links.yaml with proper entities list format
    fn setup_entity_project(tmp: &TempDir, name: &str) -> std::path::PathBuf {
        let project = tmp.path().join(name);
        std::fs::create_dir_all(project.join("src/entities")).unwrap();
        std::fs::create_dir_all(project.join("config")).unwrap();

        // Cargo.toml with this dependency
        let cargo_toml = format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
this = "0.0.8"
tokio = {{ version = "1", features = ["full"] }}
"#
        );
        std::fs::write(project.join("Cargo.toml"), cargo_toml).unwrap();

        // src/stores.rs with markers (mimics the template output)
        let stores_rs = format!(
            r#"#[allow(unused_imports)]
use std::sync::Arc;

use this::prelude::*;

pub trait EntityStore: EntityFetcher + EntityCreator + Send + Sync {{}}
impl<T> EntityStore for T where T: EntityFetcher + EntityCreator + Send + Sync {{}}

pub struct {pascal}Stores {{
    // [this:store_fields]
}}

impl {pascal}Stores {{
    pub fn new_in_memory() -> Self {{
        // [this:store_init_vars]

        Self {{
            // [this:store_init_fields]
        }}
    }}
}}
"#,
            pascal = "Test"
        );
        std::fs::write(project.join("src/stores.rs"), stores_rs).unwrap();

        // src/module.rs with markers (mimics the template output)
        let module_rs = format!(
            r#"use std::sync::Arc;

use this::core::module::Module;
use this::prelude::*;
use this::server::entity_registry::EntityRegistry;

// [this:module_imports]

use crate::stores::TestStores;

pub struct TestModule {{
    pub stores: TestStores,
}}

impl TestModule {{
    pub fn new(stores: TestStores) -> Self {{
        Self {{ stores }}
    }}
}}

impl Module for TestModule {{
    fn name(&self) -> &str {{
        "{name}"
    }}

    fn entity_types(&self) -> Vec<&str> {{
        vec![
            // [this:entity_types]
        ]
    }}

    fn links_config(&self) -> anyhow::Result<LinksConfig> {{
        let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/config/links.yaml");
        LinksConfig::from_yaml_file(config_path)
    }}

    fn register_entities(&self, _registry: &mut EntityRegistry) {{
        // [this:register_entities]
    }}

    fn get_entity_fetcher(&self, _entity_type: &str) -> Option<Arc<dyn EntityFetcher>> {{
        match _entity_type {{
            // [this:entity_fetcher]
            _ => None,
        }}
    }}

    fn get_entity_creator(&self, _entity_type: &str) -> Option<Arc<dyn EntityCreator>> {{
        match _entity_type {{
            // [this:entity_creator]
            _ => None,
        }}
    }}
}}
"#,
            name = name
        );
        std::fs::write(project.join("src/module.rs"), module_rs).unwrap();

        // config/links.yaml
        std::fs::write(
            project.join("config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        project
    }

    /// Create a workspace scaffold with API subdirectory for entity testing.
    fn setup_entity_workspace(tmp: &TempDir, name: &str) -> std::path::PathBuf {
        let ws = tmp.path().join(name);
        std::fs::create_dir_all(ws.join("api/src/entities")).unwrap();
        std::fs::create_dir_all(ws.join("api/config")).unwrap();

        // this.yaml
        let this_yaml = format!("name: {name}\napi:\n  path: api\n  port: 3000\ntargets: []\n");
        std::fs::write(ws.join("this.yaml"), this_yaml).unwrap();

        // Workspace Cargo.toml
        std::fs::write(
            ws.join("Cargo.toml"),
            "[workspace]\nmembers = [\"api\"]\nresolver = \"2\"\n",
        )
        .unwrap();

        // API Cargo.toml
        let api_cargo = format!(
            r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
this = "0.0.8"
tokio = {{ version = "1", features = ["full"] }}
"#
        );
        std::fs::write(ws.join("api/Cargo.toml"), api_cargo).unwrap();

        // src/stores.rs with markers
        let stores_rs = format!(
            r#"#[allow(unused_imports)]
use std::sync::Arc;

use this::prelude::*;

pub trait EntityStore: EntityFetcher + EntityCreator + Send + Sync {{}}
impl<T> EntityStore for T where T: EntityFetcher + EntityCreator + Send + Sync {{}}

pub struct {pascal}Stores {{
    // [this:store_fields]
}}

impl {pascal}Stores {{
    pub fn new_in_memory() -> Self {{
        // [this:store_init_vars]

        Self {{
            // [this:store_init_fields]
        }}
    }}
}}
"#,
            pascal = "Test"
        );
        std::fs::write(ws.join("api/src/stores.rs"), stores_rs).unwrap();

        // src/module.rs with markers
        let module_rs = format!(
            r#"use std::sync::Arc;

use this::core::module::Module;
use this::prelude::*;
use this::server::entity_registry::EntityRegistry;

// [this:module_imports]

use crate::stores::TestStores;

pub struct TestModule {{
    pub stores: TestStores,
}}

impl TestModule {{
    pub fn new(stores: TestStores) -> Self {{
        Self {{ stores }}
    }}
}}

impl Module for TestModule {{
    fn name(&self) -> &str {{
        "{name}"
    }}

    fn entity_types(&self) -> Vec<&str> {{
        vec![
            // [this:entity_types]
        ]
    }}

    fn links_config(&self) -> anyhow::Result<LinksConfig> {{
        let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/config/links.yaml");
        LinksConfig::from_yaml_file(config_path)
    }}

    fn register_entities(&self, _registry: &mut EntityRegistry) {{
        // [this:register_entities]
    }}

    fn get_entity_fetcher(&self, _entity_type: &str) -> Option<Arc<dyn EntityFetcher>> {{
        match _entity_type {{
            // [this:entity_fetcher]
            _ => None,
        }}
    }}

    fn get_entity_creator(&self, _entity_type: &str) -> Option<Arc<dyn EntityCreator>> {{
        match _entity_type {{
            // [this:entity_creator]
            _ => None,
        }}
    }}
}}
"#,
            name = name
        );
        std::fs::write(ws.join("api/src/module.rs"), module_rs).unwrap();

        // config/links.yaml
        std::fs::write(
            ws.join("api/config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();

        ws
    }

    fn default_args(name: &str) -> super::super::AddEntityArgs {
        super::super::AddEntityArgs {
            name: name.to_string(),
            fields: None,
            validated: false,
            indexed: "name".to_string(),
            backend: "in-memory".to_string(),
        }
    }

    // ========================================================================
    // run_in() tests
    // ========================================================================

    #[test]
    fn test_add_entity_creates_files() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "creates_files");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        // Verify all entity files were created
        assert_file_exists(&project, "src/entities/product/model.rs");
        assert_file_exists(&project, "src/entities/product/store.rs");
        assert_file_exists(&project, "src/entities/product/handlers.rs");
        assert_file_exists(&project, "src/entities/product/descriptor.rs");
        assert_file_exists(&project, "src/entities/product/mod.rs");
    }

    #[test]
    fn test_add_entity_updates_entities_mod() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "updates_mod");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        // The entities/mod.rs should be created with the pub mod declaration
        assert_file_exists(&project, "src/entities/mod.rs");
        assert_file_contains(&project, "src/entities/mod.rs", "pub mod product;");
    }

    #[test]
    fn test_add_entity_updates_existing_entities_mod() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "existing_mod");
        // Pre-create entities/mod.rs with some existing content
        std::fs::write(project.join("src/entities/mod.rs"), "pub mod order;\n").unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        assert_file_contains(&project, "src/entities/mod.rs", "pub mod order;");
        assert_file_contains(&project, "src/entities/mod.rs", "pub mod product;");
    }

    #[test]
    fn test_add_entity_updates_stores() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "updates_stores");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        // stores.rs should have the new store fields
        assert_file_contains(&project, "src/stores.rs", "products_store:");
        assert_file_contains(&project, "src/stores.rs", "products_entity:");
        // In-memory backend should have InMemoryProductStore init
        assert_file_contains(&project, "src/stores.rs", "InMemoryProductStore");
    }

    #[test]
    fn test_add_entity_updates_module_rs() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "updates_module");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        // module.rs should register the entity
        assert_file_contains(&project, "src/module.rs", "\"product\"");
        assert_file_contains(&project, "src/module.rs", "ProductDescriptor");
    }

    #[test]
    fn test_add_entity_updates_links_yaml() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "updates_links");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        // links.yaml should have the entity entry
        assert_file_contains(&project, "config/links.yaml", "product");
        assert_file_contains(&project, "config/links.yaml", "products");
    }

    #[test]
    fn test_add_entity_with_fields() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "with_fields");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.fields = Some("sku:String,price:f64".to_string());

        run_in(args, &writer, &project).unwrap();

        // The model should contain the custom fields
        assert_file_contains(&project, "src/entities/product/model.rs", "sku");
        assert_file_contains(&project, "src/entities/product/model.rs", "price");
    }

    #[test]
    fn test_add_entity_with_optional_fields() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "optional_fields");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.fields = Some("description:Option<String>".to_string());

        run_in(args, &writer, &project).unwrap();

        assert_file_contains(&project, "src/entities/product/model.rs", "description");
        assert_file_contains(&project, "src/entities/product/model.rs", "Option<String>");
    }

    #[test]
    fn test_add_entity_postgres_backend() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "pg_backend");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.backend = "postgres".to_string();

        run_in(args, &writer, &project).unwrap();

        // store.rs should use postgres template
        assert_file_contains(&project, "src/entities/product/store.rs", "Postgres");
        // stores.rs should have postgres constructor/imports
        assert_file_contains(&project, "src/stores.rs", "PostgresProductStore");
        // Migration should be generated
        assert_dir_exists(&project, "migrations");
    }

    #[test]
    fn test_add_entity_lmdb_backend() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "lmdb_backend");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.backend = "lmdb".to_string();

        run_in(args, &writer, &project).unwrap();

        // store.rs should use lmdb template
        assert_file_contains(&project, "src/entities/product/store.rs", "Lmdb");
        // stores.rs should have lmdb constructor/imports
        assert_file_contains(&project, "src/stores.rs", "LmdbProductStore");
    }

    #[test]
    fn test_add_entity_duplicate_error() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "dup_entity");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // First time: success
        let args = default_args("product");
        run_in(args, &writer, &project).unwrap();

        // Second time: should fail because entity dir already exists
        let args2 = default_args("product");
        let result = run_in(args2, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention duplicate: {}",
            err
        );
    }

    #[test]
    fn test_add_entity_outside_project_error() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        // Pass a path with no Cargo.toml/this.yaml anywhere
        let result = run_in(args, &writer, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Not inside a this-rs project"),
            "Error should mention not in project: {}",
            err
        );
    }

    #[test]
    fn test_add_entity_with_validated_flag() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "validated");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.validated = true;

        run_in(args, &writer, &project).unwrap();

        // The model should use the validated template
        assert_file_contains(&project, "src/entities/product/model.rs", "validated");
    }

    #[test]
    fn test_add_entity_with_custom_indexed_fields() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "indexed");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.indexed = "sku,name".to_string();
        args.fields = Some("sku:String".to_string());

        run_in(args, &writer, &project).unwrap();

        // The store should contain the indexed fields
        assert_file_contains(&project, "src/entities/product/store.rs", "sku");
        assert_file_contains(&project, "src/entities/product/store.rs", "name");
    }

    #[test]
    fn test_add_entity_in_workspace() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_entity_workspace(&tmp, "ws_entity");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        // Run from workspace root — should resolve to api/ subdirectory
        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        // Entity files should be under api/src/entities/
        assert_file_exists(&ws, "api/src/entities/product/model.rs");
        assert_file_exists(&ws, "api/src/entities/product/store.rs");
        assert_file_exists(&ws, "api/src/entities/product/handlers.rs");
        assert_file_exists(&ws, "api/src/entities/product/descriptor.rs");
        assert_file_exists(&ws, "api/src/entities/product/mod.rs");
        assert_file_contains(&ws, "api/src/entities/mod.rs", "pub mod product;");
    }

    #[test]
    fn test_add_entity_model_content() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "model_content");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        // Model should contain entity struct and impl_data_entity! macro
        assert_file_contains(&project, "src/entities/product/model.rs", "Product");
        assert_file_contains(
            &project,
            "src/entities/product/model.rs",
            "impl_data_entity",
        );
    }

    #[test]
    fn test_add_entity_descriptor_content() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "desc_content");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        // Descriptor should reference the entity
        assert_file_contains(&project, "src/entities/product/descriptor.rs", "Product");
        assert_file_contains(&project, "src/entities/product/descriptor.rs", "product");
    }

    #[test]
    fn test_add_entity_mod_rs_content() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "mod_content");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        run_in(args, &writer, &project).unwrap();

        // Entity mod.rs should re-export submodules
        assert_file_contains(&project, "src/entities/product/mod.rs", "pub mod model");
        assert_file_contains(&project, "src/entities/product/mod.rs", "pub mod store");
        assert_file_contains(&project, "src/entities/product/mod.rs", "pub mod handlers");
        assert_file_contains(
            &project,
            "src/entities/product/mod.rs",
            "pub mod descriptor",
        );
    }

    #[test]
    fn test_add_entity_mysql_backend() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "mysql_backend");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.backend = "mysql".to_string();

        run_in(args, &writer, &project).unwrap();

        assert_file_contains(&project, "src/entities/product/store.rs", "Mysql");
        assert_file_contains(&project, "src/stores.rs", "MysqlProductStore");
        // Migration should be generated for mysql
        assert_dir_exists(&project, "migrations");
    }

    #[test]
    fn test_add_entity_mongodb_backend() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "mongo_backend");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        args.backend = "mongodb".to_string();

        run_in(args, &writer, &project).unwrap();

        assert_file_contains(&project, "src/entities/product/store.rs", "Mongo");
        assert_file_contains(&project, "src/stores.rs", "MongoProductStore");
    }

    #[test]
    fn test_add_entity_reserved_fields_skipped() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "reserved_fields");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = default_args("product");
        // "name" and "id" are reserved, "sku" is custom
        args.fields = Some("id:Uuid,name:String,sku:String".to_string());

        run_in(args, &writer, &project).unwrap();

        // Only the custom field should appear (reserved fields are filtered out)
        let model_content =
            std::fs::read_to_string(project.join("src/entities/product/model.rs")).unwrap();
        assert!(
            model_content.contains("sku"),
            "Custom field 'sku' should be present"
        );
    }

    #[test]
    fn test_add_entity_pascal_case_naming() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "pascal_name");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        // Pass PascalCase name — should be converted to snake_case for dir/files
        let args = default_args("OrderItem");

        run_in(args, &writer, &project).unwrap();

        // Directory should use snake_case
        assert_file_exists(&project, "src/entities/order_item/model.rs");
        // Model should use PascalCase for the struct
        assert_file_contains(&project, "src/entities/order_item/model.rs", "OrderItem");
        // entities/mod.rs should use snake_case
        assert_file_contains(&project, "src/entities/mod.rs", "pub mod order_item;");
    }

    #[test]
    fn test_add_entity_stores_idempotent() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "idempotent");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // Add first entity
        let args1 = default_args("product");
        run_in(args1, &writer, &project).unwrap();

        // Add second entity (different name, should work)
        let args2 = default_args("order");
        run_in(args2, &writer, &project).unwrap();

        // Both should appear in stores.rs
        assert_file_contains(&project, "src/stores.rs", "products_store:");
        assert_file_contains(&project, "src/stores.rs", "orders_store:");
        // Both should appear in module.rs
        assert_file_contains(&project, "src/module.rs", "\"product\"");
        assert_file_contains(&project, "src/module.rs", "\"order\"");
    }

    #[test]
    fn test_add_entity_no_stores_rs() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "no_stores");
        // Remove stores.rs
        std::fs::remove_file(project.join("src/stores.rs")).unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        // Should succeed even without stores.rs (just skips that update)
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());
        assert_file_exists(&project, "src/entities/product/model.rs");
    }

    #[test]
    fn test_add_entity_no_module_rs() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "no_module");
        // Remove module.rs
        std::fs::remove_file(project.join("src/module.rs")).unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        // Should succeed even without module.rs (just skips that update)
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());
        assert_file_exists(&project, "src/entities/product/model.rs");
    }

    #[test]
    fn test_add_entity_no_links_yaml() {
        let tmp = TempDir::new().unwrap();
        let project = setup_entity_project(&tmp, "no_links");
        // Remove links.yaml
        std::fs::remove_file(project.join("config/links.yaml")).unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = default_args("product");

        // Should succeed even without links.yaml (just skips that update)
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());
        assert_file_exists(&project, "src/entities/product/model.rs");
    }

    // ========================================================================
    // Existing parse_fields tests (preserved)
    // ========================================================================

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
