use std::collections::HashMap;

use anyhow::{Context, Result};
use tera::Tera;

use crate::utils::naming;

// ============================================================================
// Project Templates
// ============================================================================

const TPL_PROJECT_CARGO_TOML: &str = include_str!("project/Cargo.toml.tera");
const TPL_PROJECT_MAIN_RS: &str = include_str!("project/main.rs.tera");
const TPL_PROJECT_MODULE_RS: &str = include_str!("project/module.rs.tera");
const TPL_PROJECT_ENTITIES_MOD_RS: &str = include_str!("project/entities_mod.rs.tera");
const TPL_PROJECT_STORES_RS: &str = include_str!("project/stores.rs.tera");
const TPL_PROJECT_LINKS_YAML: &str = include_str!("project/links.yaml.tera");
const TPL_PROJECT_EMBEDDED_FRONTEND_RS: &str = include_str!("project/embedded_frontend.rs.tera");

// ============================================================================
// Workspace Templates
// ============================================================================

const TPL_WORKSPACE_THIS_YAML: &str = include_str!("workspace/this.yaml.tera");
const TPL_WORKSPACE_DOCKERFILE: &str = include_str!("workspace/Dockerfile.tera");

// ============================================================================
// Webapp Templates (React + Vite)
// ============================================================================

const TPL_WEBAPP_PACKAGE_JSON: &str = include_str!("webapp/package.json.tera");
const TPL_WEBAPP_VITE_CONFIG_TS: &str = include_str!("webapp/vite.config.ts.tera");
const TPL_WEBAPP_TSCONFIG_JSON: &str = include_str!("webapp/tsconfig.json.tera");
const TPL_WEBAPP_INDEX_HTML: &str = include_str!("webapp/index.html.tera");
const TPL_WEBAPP_MAIN_TSX: &str = include_str!("webapp/main.tsx.tera");
const TPL_WEBAPP_APP_TSX: &str = include_str!("webapp/App.tsx.tera");
const TPL_WEBAPP_APP_CSS: &str = include_str!("webapp/App.css.tera");

// ============================================================================
// Desktop Templates (Tauri 2)
// ============================================================================

const TPL_DESKTOP_TAURI_CARGO_TOML: &str = include_str!("desktop/tauri-cargo.toml.tera");
const TPL_DESKTOP_TAURI_CONF_JSON: &str = include_str!("desktop/tauri.conf.json.tera");
const TPL_DESKTOP_TAURI_MAIN_RS: &str = include_str!("desktop/tauri-main.rs.tera");
const TPL_DESKTOP_TAURI_BUILD_RS: &str = include_str!("desktop/tauri-build.rs.tera");
const TPL_DESKTOP_CAPABILITIES_JSON: &str = include_str!("desktop/capabilities.json.tera");

// ============================================================================
// Entity Templates
// ============================================================================

const TPL_ENTITY_MODEL_RS: &str = include_str!("entity/model.rs.tera");
const TPL_ENTITY_MODEL_VALIDATED_RS: &str = include_str!("entity/model_validated.rs.tera");
const TPL_ENTITY_STORE_RS: &str = include_str!("entity/store.rs.tera");
const TPL_ENTITY_HANDLERS_RS: &str = include_str!("entity/handlers.rs.tera");
const TPL_ENTITY_DESCRIPTOR_RS: &str = include_str!("entity/descriptor.rs.tera");
const TPL_ENTITY_MOD_RS: &str = include_str!("entity/mod.rs.tera");

pub struct TemplateEngine {
    tera: Tera,
}

impl TemplateEngine {
    pub fn new() -> Result<Self> {
        let mut tera = Tera::default();

        // Register all templates
        let templates: HashMap<&str, &str> = HashMap::from([
            ("project/Cargo.toml", TPL_PROJECT_CARGO_TOML),
            ("project/main.rs", TPL_PROJECT_MAIN_RS),
            ("project/module.rs", TPL_PROJECT_MODULE_RS),
            ("project/entities_mod.rs", TPL_PROJECT_ENTITIES_MOD_RS),
            ("project/stores.rs", TPL_PROJECT_STORES_RS),
            ("project/links.yaml", TPL_PROJECT_LINKS_YAML),
            (
                "project/embedded_frontend.rs",
                TPL_PROJECT_EMBEDDED_FRONTEND_RS,
            ),
            ("workspace/this.yaml", TPL_WORKSPACE_THIS_YAML),
            ("workspace/Dockerfile", TPL_WORKSPACE_DOCKERFILE),
            ("webapp/package.json", TPL_WEBAPP_PACKAGE_JSON),
            ("webapp/vite.config.ts", TPL_WEBAPP_VITE_CONFIG_TS),
            ("webapp/tsconfig.json", TPL_WEBAPP_TSCONFIG_JSON),
            ("webapp/index.html", TPL_WEBAPP_INDEX_HTML),
            ("webapp/main.tsx", TPL_WEBAPP_MAIN_TSX),
            ("webapp/App.tsx", TPL_WEBAPP_APP_TSX),
            ("webapp/App.css", TPL_WEBAPP_APP_CSS),
            ("desktop/tauri-cargo.toml", TPL_DESKTOP_TAURI_CARGO_TOML),
            ("desktop/tauri.conf.json", TPL_DESKTOP_TAURI_CONF_JSON),
            ("desktop/tauri-main.rs", TPL_DESKTOP_TAURI_MAIN_RS),
            ("desktop/tauri-build.rs", TPL_DESKTOP_TAURI_BUILD_RS),
            ("desktop/capabilities.json", TPL_DESKTOP_CAPABILITIES_JSON),
            ("entity/model.rs", TPL_ENTITY_MODEL_RS),
            ("entity/model_validated.rs", TPL_ENTITY_MODEL_VALIDATED_RS),
            ("entity/store.rs", TPL_ENTITY_STORE_RS),
            ("entity/handlers.rs", TPL_ENTITY_HANDLERS_RS),
            ("entity/descriptor.rs", TPL_ENTITY_DESCRIPTOR_RS),
            ("entity/mod.rs", TPL_ENTITY_MOD_RS),
        ]);

        for (name, content) in &templates {
            tera.add_raw_template(name, content)
                .with_context(|| format!("Failed to register template: {}", name))?;
        }

        // Register custom filters
        tera.register_filter("snake_case", tera_filter_snake_case);
        tera.register_filter("pascal_case", tera_filter_pascal_case);
        tera.register_filter("pluralize", tera_filter_pluralize);

        Ok(Self { tera })
    }

    pub fn render(&self, template_name: &str, context: &tera::Context) -> Result<String> {
        self.tera
            .render(template_name, context)
            .with_context(|| format!("Failed to render template: {}", template_name))
    }
}

// ============================================================================
// Tera Custom Filters
// ============================================================================

fn tera_filter_snake_case(
    value: &tera::Value,
    _args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let s = value
        .as_str()
        .ok_or_else(|| tera::Error::msg("snake_case filter expects a string"))?;
    Ok(tera::Value::String(naming::to_snake_case(s)))
}

fn tera_filter_pascal_case(
    value: &tera::Value,
    _args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let s = value
        .as_str()
        .ok_or_else(|| tera::Error::msg("pascal_case filter expects a string"))?;
    Ok(tera::Value::String(naming::to_pascal_case(s)))
}

fn tera_filter_pluralize(
    value: &tera::Value,
    _args: &HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let s = value
        .as_str()
        .ok_or_else(|| tera::Error::msg("pluralize filter expects a string"))?;
    Ok(tera::Value::String(naming::pluralize(s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project_context() -> tera::Context {
        let mut ctx = tera::Context::new();
        ctx.insert("project_name", "test-project");
        ctx.insert("project_name_snake", "test_project");
        ctx.insert("port", &3000u16);
        ctx
    }

    fn make_entity_context() -> tera::Context {
        let mut ctx = tera::Context::new();
        ctx.insert("entity_name", "product");
        ctx.insert("entity_pascal", "Product");
        ctx.insert("entity_plural", "products");
        ctx.insert("validated", &false);
        ctx.insert("indexed_fields", &vec!["name".to_string()]);

        #[derive(serde::Serialize)]
        struct Field {
            name: String,
            rust_type: String,
            is_optional: bool,
        }
        ctx.insert(
            "fields",
            &vec![
                Field {
                    name: "sku".into(),
                    rust_type: "String".into(),
                    is_optional: false,
                },
                Field {
                    name: "price".into(),
                    rust_type: "f64".into(),
                    is_optional: false,
                },
                Field {
                    name: "description".into(),
                    rust_type: "Option<String>".into(),
                    is_optional: true,
                },
            ],
        );
        ctx
    }

    #[test]
    fn test_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(
            engine.is_ok(),
            "TemplateEngine should initialize without errors"
        );
    }

    #[test]
    fn test_project_cargo_toml() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/Cargo.toml", &make_project_context());
        assert!(result.is_ok(), "Cargo.toml template should render");
        let content = result.unwrap();
        assert!(content.contains("name = \"test-project\""));
        assert!(content.contains("this = "));
        // No unresolved placeholders
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_project_main_rs() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/main.rs", &make_project_context());
        assert!(result.is_ok(), "main.rs template should render");
        let content = result.unwrap();
        assert!(content.contains("TestProjectModule"));
        assert!(content.contains("TestProjectStores"));
        assert!(content.contains("Stores::new_in_memory()"));
        assert!(content.contains("Module::new(stores)"));
        assert!(content.contains("127.0.0.1:3000"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_project_module_rs() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/module.rs", &make_project_context());
        assert!(result.is_ok(), "module.rs template should render");
        let content = result.unwrap();
        assert!(content.contains("TestProjectModule"));
        assert!(content.contains("impl Module for"));
        // Markers for automated entity registration
        assert!(content.contains("[this:entity_types]"));
        assert!(content.contains("[this:register_entities]"));
        assert!(content.contains("[this:entity_fetcher]"));
        assert!(content.contains("[this:entity_creator]"));
        assert!(content.contains("[this:module_imports]"));
        // Stores integration
        assert!(content.contains("stores: TestProjectStores"));
        assert!(content.contains("fn new(stores: TestProjectStores)"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_project_links_yaml() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/links.yaml", &make_project_context());
        assert!(result.is_ok(), "links.yaml template should render");
        let content = result.unwrap();
        assert!(content.contains("entities:"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_entity_model() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("entity/model.rs", &make_entity_context());
        assert!(
            result.is_ok(),
            "entity model template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("impl_data_entity!"));
        assert!(content.contains("Product"));
        assert!(content.contains("sku: String"));
        assert!(content.contains("price: f64"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_entity_model_validated() {
        let mut ctx = make_entity_context();
        ctx.insert("validated", &true);
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("entity/model_validated.rs", &ctx);
        assert!(
            result.is_ok(),
            "validated model template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("impl_data_entity_validated!"));
        assert!(content.contains("validate:"));
        assert!(content.contains("filters:"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_entity_store() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("entity/store.rs", &make_entity_context());
        assert!(
            result.is_ok(),
            "store template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("ProductStore"));
        assert!(content.contains("InMemoryProductStore"));
        assert!(content.contains("ProductStoreError"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_entity_handlers() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("entity/handlers.rs", &make_entity_context());
        assert!(
            result.is_ok(),
            "handlers template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("list_products"));
        assert!(content.contains("create_product"));
        assert!(content.contains("ProductState"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_entity_descriptor() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("entity/descriptor.rs", &make_entity_context());
        assert!(
            result.is_ok(),
            "descriptor template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("ProductDescriptor"));
        assert!(content.contains("EntityDescriptor"));
        assert!(content.contains("/products"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_project_stores_rs() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/stores.rs", &make_project_context());
        assert!(
            result.is_ok(),
            "stores.rs template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("TestProjectStores"));
        assert!(content.contains("new_in_memory"));
        assert!(content.contains("EntityStore"));
        assert!(content.contains("[this:store_fields]"));
        assert!(content.contains("[this:store_init_vars]"));
        assert!(content.contains("[this:store_init_fields]"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_entity_mod() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("entity/mod.rs", &make_entity_context());
        assert!(
            result.is_ok(),
            "entity mod template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("pub use model::Product"));
        assert!(content.contains("InMemoryProductStore"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    fn make_workspace_context() -> tera::Context {
        let mut ctx = make_project_context();
        ctx.insert("workspace", &true);
        ctx
    }

    #[test]
    fn test_cargo_toml_workspace_has_embed_feature() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/Cargo.toml", &make_workspace_context());
        assert!(result.is_ok(), "Cargo.toml workspace should render");
        let content = result.unwrap();
        assert!(
            content.contains("[features]"),
            "Should contain [features] section"
        );
        assert!(
            content.contains("embedded-frontend"),
            "Should contain embedded-frontend feature"
        );
        assert!(
            content.contains("rust-embed"),
            "Should contain rust-embed dependency"
        );
        assert!(
            content.contains("mime_guess"),
            "Should contain mime_guess dependency"
        );
        assert!(
            content.contains("tower-http"),
            "Should contain tower-http dependency"
        );
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_cargo_toml_classic_no_embed_feature() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/Cargo.toml", &make_project_context());
        assert!(result.is_ok(), "Cargo.toml classic should render");
        let content = result.unwrap();
        assert!(
            !content.contains("[features]"),
            "Classic mode should NOT contain [features]"
        );
        assert!(
            !content.contains("rust-embed"),
            "Classic mode should NOT contain rust-embed"
        );
        assert!(
            !content.contains("mime_guess"),
            "Classic mode should NOT contain mime_guess"
        );
    }

    #[test]
    fn test_embedded_frontend_renders() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/embedded_frontend.rs", &make_project_context());
        assert!(
            result.is_ok(),
            "embedded_frontend.rs should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(
            content.contains("#[derive(RustEmbed)]"),
            "Should contain RustEmbed derive"
        );
        assert!(
            content.contains("serve_embedded"),
            "Should contain serve_embedded function"
        );
        assert!(
            content.contains("index.html"),
            "Should contain SPA fallback to index.html"
        );
        assert!(
            content.contains("mime_guess"),
            "Should contain mime_guess usage"
        );
    }

    #[test]
    fn test_main_rs_workspace_has_attach_frontend() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/main.rs", &make_workspace_context());
        assert!(result.is_ok(), "main.rs workspace should render");
        let content = result.unwrap();
        assert!(
            content.contains("attach_frontend"),
            "Workspace main.rs should contain attach_frontend"
        );
        assert!(
            content.contains("mod embedded_frontend"),
            "Workspace main.rs should contain embedded_frontend module"
        );
        assert!(
            content.contains("SERVE_FRONTEND"),
            "Workspace main.rs should contain SERVE_FRONTEND env var"
        );
        assert!(
            content.contains("ServeDir"),
            "Workspace main.rs should contain ServeDir"
        );
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
        assert!(!content.contains("{%"), "No unresolved Tera blocks");
    }

    #[test]
    fn test_main_rs_classic_no_attach_frontend() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("project/main.rs", &make_project_context());
        assert!(result.is_ok(), "main.rs classic should render");
        let content = result.unwrap();
        assert!(
            !content.contains("attach_frontend"),
            "Classic main.rs should NOT contain attach_frontend"
        );
        assert!(
            !content.contains("embedded_frontend"),
            "Classic main.rs should NOT contain embedded_frontend"
        );
        assert!(
            !content.contains("ServeDir"),
            "Classic main.rs should NOT contain ServeDir"
        );
    }

    #[test]
    fn test_workspace_this_yaml() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("workspace/this.yaml", &make_project_context());
        assert!(
            result.is_ok(),
            "workspace this.yaml template should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("name: test-project"));
        assert!(content.contains("port: 3000"));
        assert!(content.contains("path: api"));
        assert!(content.contains("targets: []"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    fn make_webapp_context() -> tera::Context {
        let mut ctx = tera::Context::new();
        ctx.insert("framework", "react");
        ctx.insert("api_port", &3000u16);
        ctx.insert("project_name", "test-project");
        ctx
    }

    #[test]
    fn test_webapp_package_json() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("webapp/package.json", &make_webapp_context());
        assert!(
            result.is_ok(),
            "webapp package.json should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("\"name\": \"test-project-frontend\""));
        assert!(content.contains("\"react\""));
        assert!(content.contains("\"vite\""));
        assert!(content.contains("\"typescript\""));
    }

    #[test]
    fn test_webapp_vite_config() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("webapp/vite.config.ts", &make_webapp_context());
        assert!(
            result.is_ok(),
            "webapp vite.config.ts should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("proxy"));
        assert!(content.contains("\"/api\""));
        assert!(content.contains("http://127.0.0.1:3000"));
    }

    #[test]
    fn test_webapp_index_html() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("webapp/index.html", &make_webapp_context());
        assert!(
            result.is_ok(),
            "webapp index.html should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("<title>test-project</title>"));
        assert!(content.contains("id=\"root\""));
        assert!(content.contains("src/main.tsx"));
    }

    #[test]
    fn test_webapp_app_tsx() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("webapp/App.tsx", &make_webapp_context());
        assert!(
            result.is_ok(),
            "webapp App.tsx should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("test-project"));
        assert!(content.contains("/api/health"));
        assert!(content.contains("useEffect"));
    }

    #[test]
    fn test_webapp_all_templates_render() {
        let engine = TemplateEngine::new().unwrap();
        let ctx = make_webapp_context();
        let templates = [
            "webapp/package.json",
            "webapp/vite.config.ts",
            "webapp/tsconfig.json",
            "webapp/index.html",
            "webapp/main.tsx",
            "webapp/App.tsx",
            "webapp/App.css",
        ];
        for name in &templates {
            let result = engine.render(name, &ctx);
            assert!(
                result.is_ok(),
                "Template {} should render: {:?}",
                name,
                result.err()
            );
        }
    }

    // ========================================================================
    // Desktop (Tauri) Templates
    // ========================================================================

    fn make_desktop_context() -> tera::Context {
        let mut ctx = tera::Context::new();
        ctx.insert("project_name", "my-app");
        ctx.insert("project_name_snake", "my_app");
        ctx.insert("api_port", &3000u16);
        ctx.insert("front_path", "front");
        ctx
    }

    #[test]
    fn test_desktop_tauri_cargo_toml() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("desktop/tauri-cargo.toml", &make_desktop_context());
        assert!(
            result.is_ok(),
            "tauri Cargo.toml should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("name = \"my-app-desktop\""));
        assert!(content.contains("tauri-build"));
        assert!(content.contains("tauri = { version = \"2\""));
        assert!(content.contains("tokio"));
        assert!(content.contains("my-app = { path = \"../../api\" }"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_desktop_tauri_conf_json() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("desktop/tauri.conf.json", &make_desktop_context());
        assert!(
            result.is_ok(),
            "tauri.conf.json should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("\"productName\": \"my-app\""));
        assert!(content.contains("com.my_app.app"));
        assert!(content.contains("../../../front/dist"));
        assert!(content.contains("http://localhost:5173"));
        assert!(content.contains("1024"));
        assert!(content.contains("768"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_desktop_tauri_main_rs() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("desktop/tauri-main.rs", &make_desktop_context());
        assert!(
            result.is_ok(),
            "tauri main.rs should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("tokio::spawn"), "Should spawn API server");
        assert!(content.contains("wait_for_api"), "Should have health check");
        assert!(content.contains("tauri::Builder"), "Should build Tauri app");
        assert!(
            content.contains("start_api_server"),
            "Should have start_api_server fn"
        );
        assert!(content.contains("my_app"), "Should reference project crate");
        assert!(
            content.contains("unwrap_or(3000)"),
            "Should have default port"
        );
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_desktop_tauri_build_rs() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("desktop/tauri-build.rs", &make_desktop_context());
        assert!(
            result.is_ok(),
            "tauri build.rs should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("tauri_build::build()"));
    }

    #[test]
    fn test_desktop_capabilities_json() {
        let engine = TemplateEngine::new().unwrap();
        let result = engine.render("desktop/capabilities.json", &make_desktop_context());
        assert!(
            result.is_ok(),
            "capabilities.json should render: {:?}",
            result.err()
        );
        let content = result.unwrap();
        assert!(content.contains("core:default"));
        assert!(content.contains("shell:allow-open"));
        assert!(content.contains("my-app"));
        assert!(!content.contains("{{"), "No unresolved Tera placeholders");
    }

    #[test]
    fn test_desktop_all_templates_render() {
        let engine = TemplateEngine::new().unwrap();
        let ctx = make_desktop_context();
        let templates = [
            "desktop/tauri-cargo.toml",
            "desktop/tauri.conf.json",
            "desktop/tauri-main.rs",
            "desktop/tauri-build.rs",
            "desktop/capabilities.json",
        ];
        for name in &templates {
            let result = engine.render(name, &ctx);
            assert!(
                result.is_ok(),
                "Template {} should render: {:?}",
                name,
                result.err()
            );
            let content = result.unwrap();
            assert!(
                !content.contains("{{"),
                "Template {} has unresolved placeholders",
                name
            );
        }
    }
}
