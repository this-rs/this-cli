use std::path::Path;
use std::process::Command;

fn this_bin() -> String {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    // When built in a workspace, the binary is in the workspace target dir
    let workspace_target = manifest_dir.parent().unwrap().join("target/debug/this");
    if workspace_target.exists() {
        return workspace_target.to_string_lossy().to_string();
    }
    // Fallback to crate-level target
    manifest_dir
        .join("target/debug/this")
        .to_string_lossy()
        .to_string()
}

fn run_this(args: &[&str], cwd: &Path) -> (bool, String, String) {
    let output = Command::new(this_bin())
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("Failed to execute this CLI");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.success(), stdout, stderr)
}

// ============================================================================
// this init tests
// ============================================================================

#[test]
fn test_init_creates_project_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["init", "my-api"], tmp.path());

    assert!(success, "init should succeed");
    assert!(stdout.contains("Project 'my-api' created successfully"));

    let project_dir = tmp.path().join("my-api");
    assert!(project_dir.join("Cargo.toml").exists());
    assert!(project_dir.join("src/main.rs").exists());
    assert!(project_dir.join("src/module.rs").exists());
    assert!(project_dir.join("src/entities/mod.rs").exists());
    assert!(project_dir.join("config/links.yaml").exists());
    assert!(project_dir.join(".gitignore").exists());
    assert!(project_dir.join(".git").exists());
}

#[test]
fn test_init_no_git() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(&["init", "no-git-project", "--no-git"], tmp.path());

    assert!(success);
    let project_dir = tmp.path().join("no-git-project");
    assert!(project_dir.join("Cargo.toml").exists());
    assert!(!project_dir.join(".git").exists());
    assert!(!project_dir.join(".gitignore").exists());
}

#[test]
fn test_init_custom_port() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(&["init", "custom-port", "--port", "8080"], tmp.path());

    assert!(success);
    let main_rs = std::fs::read_to_string(tmp.path().join("custom-port/src/main.rs")).unwrap();
    assert!(main_rs.contains("8080"));
    assert!(!main_rs.contains("3000"));
}

#[test]
fn test_init_directory_exists_error() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("existing")).unwrap();

    let (success, _, stderr) = run_this(&["init", "existing"], tmp.path());

    assert!(!success, "init should fail when directory exists");
    assert!(stderr.contains("already exists"));
}

#[test]
fn test_init_generated_cargo_toml_valid() {
    let tmp = tempfile::tempdir().unwrap();
    run_this(&["init", "toml-test"], tmp.path());

    let content = std::fs::read_to_string(tmp.path().join("toml-test/Cargo.toml")).unwrap();
    // Parse as TOML to verify validity
    let parsed: toml_edit::DocumentMut = content
        .parse()
        .expect("Generated Cargo.toml should be valid TOML");
    assert_eq!(parsed["package"]["name"].as_str().unwrap(), "toml-test");
}

// ============================================================================
// this init --workspace tests
// ============================================================================

#[test]
fn test_init_workspace_creates_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["init", "my-ws", "--workspace"], tmp.path());

    assert!(success, "init --workspace should succeed");
    assert!(stdout.contains("Workspace 'my-ws' created successfully"));

    let ws_dir = tmp.path().join("my-ws");

    // Workspace root files
    assert!(ws_dir.join("this.yaml").exists(), "this.yaml should exist");
    assert!(
        ws_dir.join(".gitignore").exists(),
        ".gitignore should exist"
    );
    assert!(
        ws_dir.join(".git").exists(),
        "git repo should be initialized"
    );

    // Verify this.yaml content
    let this_yaml = std::fs::read_to_string(ws_dir.join("this.yaml")).unwrap();
    assert!(
        this_yaml.contains("name: my-ws"),
        "this.yaml should contain project name"
    );
    assert!(
        this_yaml.contains("api:"),
        "this.yaml should contain api section"
    );

    // API subdirectory with classic scaffold
    assert!(
        ws_dir.join("api/Cargo.toml").exists(),
        "api/Cargo.toml should exist"
    );
    assert!(
        ws_dir.join("api/src/main.rs").exists(),
        "api/src/main.rs should exist"
    );
    assert!(
        ws_dir.join("api/src/module.rs").exists(),
        "api/src/module.rs should exist"
    );
    assert!(
        ws_dir.join("api/src/entities/mod.rs").exists(),
        "api/src/entities/mod.rs should exist"
    );
    assert!(
        ws_dir.join("api/config/links.yaml").exists(),
        "api/config/links.yaml should exist"
    );
    assert!(
        ws_dir.join("api/dist/.gitkeep").exists(),
        "api/dist/.gitkeep should exist"
    );

    // Embedded frontend support files
    assert!(
        ws_dir.join("api/src/embedded_frontend.rs").exists(),
        "api/src/embedded_frontend.rs should exist"
    );

    // Verify embedded_frontend.rs content
    let ef_content = std::fs::read_to_string(ws_dir.join("api/src/embedded_frontend.rs")).unwrap();
    assert!(
        ef_content.contains("RustEmbed"),
        "embedded_frontend.rs should contain RustEmbed"
    );
    assert!(
        ef_content.contains("serve_embedded"),
        "embedded_frontend.rs should contain serve_embedded"
    );

    // Verify Cargo.toml has embed features
    let cargo_toml = std::fs::read_to_string(ws_dir.join("api/Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains("[features]"),
        "Cargo.toml should have [features] section"
    );
    assert!(
        cargo_toml.contains("embedded-frontend"),
        "Cargo.toml should have embedded-frontend feature"
    );

    // Verify main.rs has attach_frontend
    let main_rs = std::fs::read_to_string(ws_dir.join("api/src/main.rs")).unwrap();
    assert!(
        main_rs.contains("attach_frontend"),
        "main.rs should contain attach_frontend"
    );

    // Workspace .gitignore should include frontend artifacts
    let gitignore = std::fs::read_to_string(ws_dir.join(".gitignore")).unwrap();
    assert!(
        gitignore.contains("node_modules/"),
        ".gitignore should contain node_modules"
    );
    assert!(
        gitignore.contains("dist/"),
        ".gitignore should contain dist/"
    );
}

#[test]
fn test_init_classic_unchanged() {
    // Regression test: classic init (without --workspace) MUST produce the same result
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["init", "classic-proj", "--no-git"], tmp.path());

    assert!(success, "classic init should succeed");
    assert!(stdout.contains("Project 'classic-proj' created successfully"));

    let project_dir = tmp.path().join("classic-proj");

    // Classic mode should NOT have embed features
    let cargo_toml = std::fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
    assert!(
        !cargo_toml.contains("[features]"),
        "Classic Cargo.toml should NOT have [features]"
    );
    assert!(
        !cargo_toml.contains("rust-embed"),
        "Classic Cargo.toml should NOT have rust-embed"
    );
    let main_rs = std::fs::read_to_string(project_dir.join("src/main.rs")).unwrap();
    assert!(
        !main_rs.contains("attach_frontend"),
        "Classic main.rs should NOT have attach_frontend"
    );
    assert!(
        !project_dir.join("src/embedded_frontend.rs").exists(),
        "Classic mode should NOT have embedded_frontend.rs"
    );

    // Classic structure: flat, no this.yaml, no api/ subdirectory
    assert!(
        project_dir.join("Cargo.toml").exists(),
        "Cargo.toml at root"
    );
    assert!(
        project_dir.join("src/main.rs").exists(),
        "src/main.rs at root"
    );
    assert!(
        !project_dir.join("this.yaml").exists(),
        "No this.yaml in classic mode"
    );
    assert!(
        !project_dir.join("api").exists(),
        "No api/ directory in classic mode"
    );
}

#[test]
fn test_init_workspace_dry_run() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(
        &["--dry-run", "init", "phantom-ws", "--workspace"],
        tmp.path(),
    );

    assert!(success, "dry-run init --workspace should succeed");
    assert!(stdout.contains("Dry run"), "Should show dry-run banner");
    assert!(stdout.contains("Would create"), "Should list files");
    assert!(
        !tmp.path().join("phantom-ws").exists(),
        "Should NOT create the directory"
    );
}

#[test]
fn test_init_workspace_custom_port() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "port-ws", "--workspace", "--port", "8080"],
        tmp.path(),
    );

    assert!(success);
    let ws_dir = tmp.path().join("port-ws");

    // this.yaml should have custom port
    let this_yaml = std::fs::read_to_string(ws_dir.join("this.yaml")).unwrap();
    assert!(
        this_yaml.contains("8080"),
        "this.yaml should contain custom port"
    );
    assert!(
        !this_yaml.contains("3000"),
        "this.yaml should NOT contain default port"
    );

    // api/src/main.rs should also have custom port
    let main_rs = std::fs::read_to_string(ws_dir.join("api/src/main.rs")).unwrap();
    assert!(
        main_rs.contains("8080"),
        "main.rs should contain custom port"
    );
}

#[test]
fn test_init_workspace_directory_exists_error() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("existing-ws")).unwrap();

    let (success, _, stderr) = run_this(&["init", "existing-ws", "--workspace"], tmp.path());

    assert!(
        !success,
        "init --workspace should fail when directory exists"
    );
    assert!(stderr.contains("already exists"));
}

#[test]
fn test_workspace_add_entity_from_root() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "ws-entity", "--workspace", "--no-git"],
        tmp.path(),
    );
    assert!(success, "workspace init should succeed");

    let ws_dir = tmp.path().join("ws-entity");

    // Add entity from workspace root — should resolve to api/ via detect_project_root()
    let (success, stdout, stderr) = run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "sku:String,price:f64",
        ],
        &ws_dir,
    );

    assert!(
        success,
        "add entity from workspace root should succeed. stderr: {}",
        stderr
    );
    assert!(stdout.contains("Entity 'product' created"));

    // Entity files should be in api/src/entities/product/
    assert!(ws_dir.join("api/src/entities/product/model.rs").exists());
    assert!(ws_dir.join("api/src/entities/product/store.rs").exists());
    assert!(ws_dir.join("api/src/entities/product/handlers.rs").exists());
}

#[test]
fn test_workspace_info_from_root() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(&["init", "ws-info", "--workspace", "--no-git"], tmp.path());
    assert!(success);

    let ws_dir = tmp.path().join("ws-info");

    // Info from workspace root should work
    let (success, stdout, stderr) = run_this(&["info"], &ws_dir);

    assert!(
        success,
        "info from workspace root should succeed. stderr: {}",
        stderr
    );
    assert!(stdout.contains("Project:"), "Should show project info");
}

// ============================================================================
// this add entity tests
// ============================================================================

fn setup_project(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    let (success, _, _) = run_this(&["init", "test-proj", "--no-git"], tmp.path());
    assert!(success, "Project init should succeed");
    tmp.path().join("test-proj")
}

#[test]
fn test_add_entity_creates_files() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, stdout, _) = run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "sku:String,price:f64",
        ],
        &project,
    );

    assert!(success, "add entity should succeed");
    assert!(stdout.contains("Entity 'product' created"));

    let entity_dir = project.join("src/entities/product");
    assert!(entity_dir.join("model.rs").exists());
    assert!(entity_dir.join("store.rs").exists());
    assert!(entity_dir.join("handlers.rs").exists());
    assert!(entity_dir.join("descriptor.rs").exists());
    assert!(entity_dir.join("mod.rs").exists());
}

#[test]
fn test_add_entity_updates_mod_rs() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(&["add", "entity", "product"], &project);

    let mod_content = std::fs::read_to_string(project.join("src/entities/mod.rs")).unwrap();
    assert!(mod_content.contains("pub mod product;"));
}

#[test]
fn test_add_entity_validated_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, _, _) = run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "price:f64",
            "--validated",
        ],
        &project,
    );

    assert!(success);
    let model = std::fs::read_to_string(project.join("src/entities/product/model.rs")).unwrap();
    assert!(model.contains("impl_data_entity_validated!"));
    assert!(model.contains("validate:"));
    assert!(model.contains("filters:"));
}

#[test]
fn test_add_entity_without_validated() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, _, _) = run_this(
        &["add", "entity", "category", "--fields", "slug:String"],
        &project,
    );

    assert!(success);
    let model = std::fs::read_to_string(project.join("src/entities/category/model.rs")).unwrap();
    assert!(model.contains("impl_data_entity!"));
    assert!(!model.contains("impl_data_entity_validated!"));
}

#[test]
fn test_add_entity_duplicate_error() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(&["add", "entity", "product"], &project);
    let (success, _, stderr) = run_this(&["add", "entity", "product"], &project);

    assert!(!success, "Duplicate entity should fail");
    assert!(stderr.contains("already exists"));
}

#[test]
fn test_add_entity_outside_project_error() {
    let tmp = tempfile::tempdir().unwrap();

    let (success, _, stderr) = run_this(&["add", "entity", "product"], tmp.path());

    assert!(!success, "Should fail outside this-rs project");
    assert!(stderr.contains("Not inside a this-rs project"));
}

// ============================================================================
// this add link tests
// ============================================================================

#[test]
fn test_add_link_basic() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, stdout, _) = run_this(&["add", "link", "order", "invoice"], &project);

    assert!(success, "add link should succeed");
    assert!(stdout.contains("Link added"));

    let yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert!(yaml.contains("has_invoice"));
    assert!(yaml.contains("source_type: order"));
    assert!(yaml.contains("target_type: invoice"));
}

#[test]
fn test_add_link_custom_options() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, _, _) = run_this(
        &[
            "add",
            "link",
            "product",
            "category",
            "--link-type",
            "belongs_to",
            "--forward",
            "parent-cats",
            "--reverse",
            "child-prods",
        ],
        &project,
    );

    assert!(success);
    let yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert!(yaml.contains("belongs_to"));
    assert!(yaml.contains("forward_route_name: parent-cats"));
    assert!(yaml.contains("reverse_route_name: child-prods"));
}

#[test]
fn test_add_link_duplicate_error() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(&["add", "link", "order", "invoice"], &project);
    let (success, _, stderr) = run_this(&["add", "link", "order", "invoice"], &project);

    assert!(!success, "Duplicate link should fail");
    assert!(stderr.contains("already exists"));
}

#[test]
fn test_add_link_no_validation_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, _, _) = run_this(
        &["add", "link", "user", "role", "--no-validation-rule"],
        &project,
    );

    assert!(success);
    let yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert!(yaml.contains("has_role"));
    // validation_rules should still be empty or not contain has_role
    let config: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let rules = config.get("validation_rules").unwrap();
    if let serde_yaml::Value::Mapping(map) = rules {
        assert!(
            !map.contains_key(serde_yaml::Value::String("has_role".into())),
            "Should not have validation rule when --no-validation-rule is set"
        );
    }
}

#[test]
fn test_add_link_adds_entity_configs() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(&["add", "link", "order", "invoice"], &project);

    let yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert!(yaml.contains("singular: order"));
    assert!(yaml.contains("singular: invoice"));
    assert!(yaml.contains("plural: orders"));
    assert!(yaml.contains("plural: invoices"));
}

// ============================================================================
// Full pipeline test
// ============================================================================

#[test]
fn test_full_pipeline() {
    let tmp = tempfile::tempdir().unwrap();

    // 1. Init project
    let (success, _, _) = run_this(&["init", "full-test", "--no-git"], tmp.path());
    assert!(success, "init should succeed");
    let project = tmp.path().join("full-test");

    // 2. Add two entities
    let (success, _, _) = run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "sku:String,price:f64",
        ],
        &project,
    );
    assert!(success, "add entity product should succeed");

    let (success, _, _) = run_this(
        &[
            "add",
            "entity",
            "category",
            "--fields",
            "slug:String,description:Option<String>",
            "--validated",
        ],
        &project,
    );
    assert!(success, "add entity category should succeed");

    // 3. Add link between them
    let (success, _, _) = run_this(&["add", "link", "product", "category"], &project);
    assert!(success, "add link should succeed");

    // Verify final state
    let mod_rs = std::fs::read_to_string(project.join("src/entities/mod.rs")).unwrap();
    assert!(mod_rs.contains("pub mod product;"));
    assert!(mod_rs.contains("pub mod category;"));

    // Product files exist
    assert!(project.join("src/entities/product/model.rs").exists());
    assert!(project.join("src/entities/product/store.rs").exists());
    assert!(project.join("src/entities/product/handlers.rs").exists());
    assert!(project.join("src/entities/product/descriptor.rs").exists());

    // Category files exist
    assert!(project.join("src/entities/category/model.rs").exists());

    // Category uses validated macro
    let cat_model =
        std::fs::read_to_string(project.join("src/entities/category/model.rs")).unwrap();
    assert!(cat_model.contains("impl_data_entity_validated!"));

    // Product uses simple macro
    let prod_model =
        std::fs::read_to_string(project.join("src/entities/product/model.rs")).unwrap();
    assert!(prod_model.contains("impl_data_entity!"));
    assert!(!prod_model.contains("impl_data_entity_validated!"));

    // Links yaml has the link
    let yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert!(yaml.contains("has_category"));
    assert!(yaml.contains("source_type: product"));
    assert!(yaml.contains("target_type: category"));
}

// ============================================================================
// Auto-update tests (stores.rs, module.rs, links.yaml)
// ============================================================================

#[test]
fn test_add_entity_updates_stores_rs() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "sku:String,price:f64",
        ],
        &project,
    );

    let stores = std::fs::read_to_string(project.join("src/stores.rs")).unwrap();
    assert!(
        stores.contains("products_store: Arc<dyn ProductStore>"),
        "Should have products_store field"
    );
    assert!(
        stores.contains("products_entity: Arc<dyn EntityStore>"),
        "Should have products_entity field"
    );
    assert!(
        stores.contains("InMemoryProductStore::default()"),
        "Should have init var"
    );
    assert!(
        stores.contains("products_store: products.clone()"),
        "Should have init field"
    );
    assert!(
        stores.contains("products_entity: products"),
        "Should have entity init field"
    );
    assert!(
        stores.contains("use crate::entities::product::{InMemoryProductStore, ProductStore};"),
        "Should have import"
    );
}

#[test]
fn test_add_entity_updates_module_rs() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "sku:String,price:f64",
        ],
        &project,
    );

    let module = std::fs::read_to_string(project.join("src/module.rs")).unwrap();
    assert!(module.contains("\"product\","), "Should have entity type");
    assert!(
        module.contains("ProductDescriptor::new_with_creator"),
        "Should have descriptor registration"
    );
    assert!(
        module.contains("\"product\" => Some(self.stores.products_entity.clone())"),
        "Should have fetcher match arm"
    );
    assert!(
        module.contains("use crate::entities::product::descriptor::ProductDescriptor;"),
        "Should have descriptor import"
    );
}

#[test]
fn test_add_entity_updates_links_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(&["add", "entity", "product"], &project);

    let yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert!(
        yaml.contains("singular: product"),
        "Should have entity config"
    );
    assert!(yaml.contains("plural: products"), "Should have plural");
}

#[test]
fn test_add_entity_idempotent_stores() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    // Add entity twice — second time the entity dir already exists so the command fails.
    // Instead, we add two different entities and check no cross-contamination.
    run_this(
        &["add", "entity", "product", "--fields", "sku:String"],
        &project,
    );
    run_this(
        &["add", "entity", "category", "--fields", "slug:String"],
        &project,
    );

    let stores = std::fs::read_to_string(project.join("src/stores.rs")).unwrap();
    // Each store field + init = 2 occurrences of "{entity}_store:"
    assert_eq!(
        stores.matches("products_store:").count(),
        2,
        "products_store should appear twice (field + init)"
    );
    assert_eq!(
        stores.matches("categories_store:").count(),
        2,
        "categories_store should appear twice (field + init)"
    );
    // InMemoryXxxStore appears in import + init = 2
    assert_eq!(
        stores.matches("InMemoryProductStore").count(),
        2,
        "InMemoryProductStore should appear twice (import + init)"
    );
    assert_eq!(
        stores.matches("InMemoryCategoryStore").count(),
        2,
        "InMemoryCategoryStore should appear twice (import + init)"
    );
}

#[test]
fn test_add_entity_idempotent_module() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(
        &["add", "entity", "product", "--fields", "sku:String"],
        &project,
    );
    run_this(
        &["add", "entity", "category", "--fields", "slug:String"],
        &project,
    );

    let module = std::fs::read_to_string(project.join("src/module.rs")).unwrap();
    // Each entity type should appear exactly once in entity_types vec
    assert_eq!(
        module.matches("\"product\",").count(),
        1,
        "product should appear once in entity_types"
    );
    assert_eq!(
        module.matches("\"category\",").count(),
        1,
        "category should appear once in entity_types"
    );
    // Each descriptor appears in import + register_entities = 2
    assert_eq!(
        module.matches("ProductDescriptor").count(),
        2,
        "ProductDescriptor should appear twice (import + register)"
    );
    assert_eq!(
        module.matches("CategoryDescriptor").count(),
        2,
        "CategoryDescriptor should appear twice (import + register)"
    );
}

#[test]
fn test_add_entity_multi_updates_stores_and_module() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    // Add three entities
    run_this(
        &["add", "entity", "product", "--fields", "sku:String"],
        &project,
    );
    run_this(
        &["add", "entity", "category", "--fields", "slug:String"],
        &project,
    );
    run_this(
        &["add", "entity", "tag", "--fields", "label:String"],
        &project,
    );

    // Verify stores has all three
    let stores = std::fs::read_to_string(project.join("src/stores.rs")).unwrap();
    assert!(stores.contains("products_store"));
    assert!(stores.contains("categories_store"));
    assert!(stores.contains("tags_store"));

    // Verify module has all three
    let module = std::fs::read_to_string(project.join("src/module.rs")).unwrap();
    assert!(module.contains("\"product\","));
    assert!(module.contains("\"category\","));
    assert!(module.contains("\"tag\","));

    // Verify links.yaml has all three
    let yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert!(yaml.contains("singular: product"));
    assert!(yaml.contains("singular: category"));
    assert!(yaml.contains("singular: tag"));
}

// ============================================================================
// this info tests
// ============================================================================

#[test]
fn test_info_in_project() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    // Add an entity so there's something to display
    run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "sku:String,price:f64",
        ],
        &project,
    );

    let (success, stdout, _) = run_this(&["info"], &project);

    assert!(success, "info should succeed");
    assert!(stdout.contains("Project:"), "Should show project name");
    assert!(stdout.contains("this-rs"), "Should show framework");
    assert!(stdout.contains("Entities"), "Should show entities section");
    assert!(stdout.contains("product"), "Should list product entity");
    assert!(stdout.contains("Status:"), "Should show status section");
}

#[test]
fn test_info_with_links() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(
        &["add", "entity", "product", "--fields", "sku:String"],
        &project,
    );
    run_this(
        &["add", "entity", "category", "--fields", "slug:String"],
        &project,
    );
    run_this(&["add", "link", "product", "category"], &project);

    let (success, stdout, _) = run_this(&["info"], &project);

    assert!(success);
    assert!(stdout.contains("Links"), "Should show links section");
    assert!(stdout.contains("has_category"), "Should show link type");
}

#[test]
fn test_info_outside_project_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, stderr) = run_this(&["info"], tmp.path());

    assert!(!success, "info should fail outside project");
    assert!(stderr.contains("Not inside a this-rs project"));
}

#[test]
fn test_info_workspace_displays_workspace_section() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "ws-info-full", "--workspace", "--no-git"],
        tmp.path(),
    );
    assert!(success);

    let ws_dir = tmp.path().join("ws-info-full");
    let (success, stdout, _) = run_this(&["info"], &ws_dir);

    assert!(success, "info should succeed in workspace");
    assert!(
        stdout.contains("Workspace:"),
        "Should show Workspace section"
    );
    assert!(
        stdout.contains("ws-info-full"),
        "Should show workspace name"
    );
    assert!(stdout.contains("API:"), "Should show API info");
    assert!(stdout.contains("Targets:"), "Should show Targets section");
}

#[test]
fn test_info_classic_no_workspace_section() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(&["init", "classic-info", "--no-git"], tmp.path());
    assert!(success);

    let project_dir = tmp.path().join("classic-info");
    let (success, stdout, _) = run_this(&["info"], &project_dir);

    assert!(success, "info should succeed in classic project");
    assert!(
        !stdout.contains("Workspace:"),
        "Should NOT show Workspace section in classic mode"
    );
}

// ============================================================================
// this doctor tests
// ============================================================================

#[test]
fn test_doctor_healthy_project() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    run_this(
        &["add", "entity", "product", "--fields", "sku:String"],
        &project,
    );

    let (success, stdout, _) = run_this(&["doctor"], &project);

    assert!(success, "doctor should succeed on healthy project");
    assert!(stdout.contains("Cargo.toml"), "Should check Cargo.toml");
    assert!(stdout.contains("passed"), "Should show summary");
}

#[test]
fn test_doctor_detects_orphan_entity() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    // Create entity directory manually without registering in mod.rs
    std::fs::create_dir_all(project.join("src/entities/ghost")).unwrap();

    let (success, stdout, _) = run_this(&["doctor"], &project);

    assert!(success, "doctor should succeed with warnings");
    assert!(
        stdout.contains("ghost"),
        "Should detect orphan entity 'ghost'"
    );
}

#[test]
fn test_doctor_detects_invalid_link_entity() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    // Add a link referencing entities that don't exist as directories
    run_this(&["add", "link", "order", "invoice"], &project);

    let (success, _stdout, _) = run_this(&["doctor"], &project);

    assert!(success, "doctor should succeed (warnings not errors)");
    // The link references order/invoice which only exist in yaml, not as entity dirs
}

#[test]
fn test_doctor_workspace_healthy() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "ws-doctor", "--workspace", "--no-git"],
        tmp.path(),
    );
    assert!(success);

    let ws_dir = tmp.path().join("ws-doctor");
    let (success, stdout, _) = run_this(&["doctor"], &ws_dir);

    assert!(success, "doctor should succeed in healthy workspace");
    assert!(
        stdout.contains("this.yaml valid"),
        "Should validate this.yaml"
    );
    assert!(
        stdout.contains("api/Cargo.toml found"),
        "Should find api/Cargo.toml"
    );
}

#[test]
fn test_doctor_workspace_missing_target_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "ws-missing", "--workspace", "--no-git"],
        tmp.path(),
    );
    assert!(success);

    let ws_dir = tmp.path().join("ws-missing");

    // Manually add a target to this.yaml that has no directory
    let this_yaml = r#"name: ws-missing
api:
  path: api
  port: 3000
targets:
  - target_type: webapp
    framework: react
    path: front
"#;
    std::fs::write(ws_dir.join("this.yaml"), this_yaml).unwrap();

    let (success, stdout, _) = run_this(&["doctor"], &ws_dir);

    assert!(success, "doctor should succeed (warnings not errors)");
    assert!(
        stdout.contains("not found"),
        "Should warn about missing target directory 'front'"
    );
    assert!(stdout.contains("webapp"), "Should mention the target type");
}

// ============================================================================
// this completions tests
// ============================================================================

#[test]
fn test_completions_bash() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["completions", "bash"], tmp.path());

    assert!(success, "completions bash should succeed");
    assert!(!stdout.is_empty(), "Should produce output");
    assert!(
        stdout.contains("_this"),
        "Should contain this completion function"
    );
}

#[test]
fn test_completions_zsh() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["completions", "zsh"], tmp.path());

    assert!(success);
    assert!(!stdout.is_empty());
}

#[test]
fn test_completions_fish() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["completions", "fish"], tmp.path());

    assert!(success);
    assert!(!stdout.is_empty());
}

#[test]
fn test_completions_powershell() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["completions", "powershell"], tmp.path());

    assert!(success);
    assert!(!stdout.is_empty());
}

// ============================================================================
// this build tests
// ============================================================================

fn setup_workspace(tmp: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let (success, _, stderr) = run_this(&["init", name, "--workspace", "--no-git"], tmp.path());
    assert!(success, "workspace init should succeed: {}", stderr);
    tmp.path().join(name)
}

#[test]
fn test_build_outside_workspace_fails() {
    let tmp = tempfile::tempdir().unwrap();

    let (success, _, stderr) = run_this(&["build"], tmp.path());

    assert!(!success, "build outside workspace should fail");
    assert!(
        stderr.contains("Not a this-rs workspace"),
        "Should say not a workspace. stderr: {}",
        stderr
    );
}

#[test]
fn test_build_in_classic_project_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, _, stderr) = run_this(&["build"], &project);

    assert!(!success, "build in classic project should fail");
    assert!(
        stderr.contains("Not a this-rs workspace"),
        "Should say not a workspace. stderr: {}",
        stderr
    );
}

#[test]
fn test_build_embed_without_webapp_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = setup_workspace(&tmp, "ws-no-webapp");

    let (success, _, stderr) = run_this(&["build", "--embed"], &ws_dir);

    assert!(!success, "build --embed without webapp should fail");
    assert!(
        stderr.contains("No webapp target configured"),
        "Should say no webapp target. stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("--embed"),
        "Should mention the flag. stderr: {}",
        stderr
    );
}

#[test]
fn test_build_front_only_without_webapp_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = setup_workspace(&tmp, "ws-no-front");

    let (success, _, stderr) = run_this(&["build", "--front-only"], &ws_dir);

    assert!(!success, "build --front-only without webapp should fail");
    assert!(
        stderr.contains("No webapp target configured"),
        "Should say no webapp target. stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("--front-only"),
        "Should mention the flag. stderr: {}",
        stderr
    );
}

#[test]
fn test_build_docker_without_webapp_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = setup_workspace(&tmp, "ws-no-docker");

    let (success, _, stderr) = run_this(&["build", "--docker"], &ws_dir);

    assert!(!success, "build --docker without webapp should fail");
    assert!(
        stderr.contains("No webapp target configured"),
        "Should say no webapp target. stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("--docker"),
        "Should mention the flag. stderr: {}",
        stderr
    );
}

#[test]
fn test_build_docker_generates_dockerfile() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = setup_workspace(&tmp, "ws-docker");

    // Manually add a webapp target to this.yaml
    let this_yaml = r#"name: ws-docker
api:
  path: api
  port: 3000
targets:
  - target_type: webapp
    framework: react
    path: front
"#;
    std::fs::write(ws_dir.join("this.yaml"), this_yaml).unwrap();

    let (success, stdout, stderr) = run_this(&["build", "--docker"], &ws_dir);

    assert!(success, "build --docker should succeed. stderr: {}", stderr);
    assert!(
        stdout.contains("Dockerfile generated"),
        "Should say Dockerfile generated. stdout: {}",
        stdout
    );

    // Verify Dockerfile was created
    let dockerfile = ws_dir.join("Dockerfile");
    assert!(dockerfile.exists(), "Dockerfile should exist");

    let content = std::fs::read_to_string(&dockerfile).unwrap();
    assert!(
        content.contains("FROM rust:1-alpine"),
        "Should contain Rust builder stage"
    );
    assert!(
        content.contains("FROM node:20-alpine"),
        "Should contain Node frontend stage"
    );
    assert!(content.contains("ws-docker"), "Should contain project name");
    assert!(content.contains("front"), "Should contain webapp path");
    assert!(content.contains("EXPOSE 3000"), "Should contain port");
}

#[test]
fn test_build_docker_dry_run() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_dir = setup_workspace(&tmp, "ws-docker-dry");

    // Manually add a webapp target
    let this_yaml = r#"name: ws-docker-dry
api:
  path: api
  port: 4000
targets:
  - target_type: webapp
    framework: react
    path: front
"#;
    std::fs::write(ws_dir.join("this.yaml"), this_yaml).unwrap();

    let (success, stdout, stderr) = run_this(&["--dry-run", "build", "--docker"], &ws_dir);

    assert!(
        success,
        "dry-run build --docker should succeed. stderr: {}",
        stderr
    );
    assert!(
        stdout.contains("Would create") || stdout.contains("Dockerfile"),
        "Should show dry-run output. stdout: {}",
        stdout
    );
    assert!(
        !ws_dir.join("Dockerfile").exists(),
        "Should NOT create Dockerfile in dry-run mode"
    );
}

// ============================================================================
// this dev tests
// ============================================================================

#[test]
fn test_dev_outside_workspace_fails() {
    let tmp = tempfile::tempdir().unwrap();

    let (success, _, stderr) = run_this(&["dev"], tmp.path());

    assert!(!success, "dev outside workspace should fail");
    assert!(
        stderr.contains("Not a this-rs workspace"),
        "Should say not a workspace. stderr: {}",
        stderr
    );
}

#[test]
fn test_dev_in_classic_project_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, _, stderr) = run_this(&["dev"], &project);

    assert!(!success, "dev in classic project should fail");
    assert!(
        stderr.contains("Not a this-rs workspace"),
        "Should say not a workspace. stderr: {}",
        stderr
    );
}

// ============================================================================
// --dry-run tests
// ============================================================================

#[test]
fn test_dry_run_init_no_files_created() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, stdout, _) = run_this(&["--dry-run", "init", "phantom"], tmp.path());

    assert!(success, "dry-run init should succeed");
    assert!(stdout.contains("Dry run"), "Should show dry-run banner");
    assert!(stdout.contains("Would create"), "Should list files");
    assert!(
        !tmp.path().join("phantom").exists(),
        "Should NOT create the directory"
    );
}

#[test]
fn test_dry_run_add_entity_no_files_created() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    let (success, stdout, _) = run_this(
        &[
            "--dry-run",
            "add",
            "entity",
            "widget",
            "--fields",
            "name:String",
        ],
        &project,
    );

    assert!(success, "dry-run add entity should succeed");
    assert!(stdout.contains("Dry run"), "Should show dry-run banner");
    assert!(
        stdout.contains("Would create"),
        "Should list files to create"
    );
    assert!(
        stdout.contains("Would modify"),
        "Should list files to modify"
    );
    assert!(
        !project.join("src/entities/widget").exists(),
        "Should NOT create entity directory"
    );
}

#[test]
fn test_dry_run_add_link_no_files_modified() {
    let tmp = tempfile::tempdir().unwrap();
    let project = setup_project(&tmp);

    // Read original links.yaml
    let original_yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();

    let (success, stdout, _) = run_this(&["--dry-run", "add", "link", "user", "role"], &project);

    assert!(success, "dry-run add link should succeed");
    assert!(stdout.contains("Dry run"), "Should show dry-run banner");

    // Verify file was NOT modified
    let after_yaml = std::fs::read_to_string(project.join("config/links.yaml")).unwrap();
    assert_eq!(
        original_yaml, after_yaml,
        "links.yaml should NOT be modified"
    );
}

// ============================================================================
// Reserved fields filtering
// ============================================================================

#[test]
fn test_add_entity_reserved_field_name_filtered() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("reserved-test");

    run_this(&["init", "reserved-test", "--no-git"], tmp.path());

    // "name" is a reserved field (built-in in impl_data_entity! macro)
    let (success, stdout, _) = run_this(
        &[
            "add",
            "entity",
            "category",
            "--fields",
            "name:String,slug:String",
        ],
        &project,
    );

    assert!(
        success,
        "add entity should succeed even with reserved fields"
    );
    assert!(
        stdout.contains("built-in") || stdout.contains("skipping"),
        "should warn about reserved field 'name'"
    );

    // Verify the generated model.rs does NOT contain 'name' in custom fields
    let model = std::fs::read_to_string(project.join("src/entities/category/model.rs")).unwrap();
    // "name" appears in indexed fields ["name"] but NOT as a custom field declaration
    assert!(
        !model.contains("        name: String,"),
        "reserved field 'name' should not appear in custom fields block. Model:\n{}",
        model
    );
}

// ============================================================================
// Compilation test (slow — requires cargo check of generated code)
// ============================================================================

#[test]
#[ignore] // Run with: cargo test -- --ignored
fn test_generated_code_compiles() {
    let tmp = tempfile::tempdir().unwrap();

    // Calculate path to the this crate relative to the generated project
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let this_crate_path = format!("{}/../this", manifest_dir);

    // Verify the this crate actually exists
    assert!(
        std::path::Path::new(&this_crate_path)
            .join("Cargo.toml")
            .exists(),
        "this crate not found at {}. This test requires the this-rs workspace layout.",
        this_crate_path
    );

    // 1. Init project with local this dependency
    let (success, _, stderr) = run_this(
        &[
            "init",
            "compile-test",
            "--no-git",
            "--this-path",
            &this_crate_path,
        ],
        tmp.path(),
    );
    assert!(success, "init should succeed: {}", stderr);
    let project = tmp.path().join("compile-test");

    // Verify the generated Cargo.toml uses path dependency
    let cargo_toml = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains("path ="),
        "Cargo.toml should use path dependency"
    );

    // 2. Add entity product with fields
    let (success, _, stderr) = run_this(
        &[
            "add",
            "entity",
            "product",
            "--fields",
            "sku:String,price:f64",
        ],
        &project,
    );
    assert!(success, "add entity product should succeed: {}", stderr);

    // 3. Add entity category with validated
    let (success, _, stderr) = run_this(
        &[
            "add",
            "entity",
            "category",
            "--fields",
            "slug:String,description:Option<String>",
            "--validated",
        ],
        &project,
    );
    assert!(success, "add entity category should succeed: {}", stderr);

    // 4. Add link between them
    let (success, _, stderr) = run_this(&["add", "link", "product", "category"], &project);
    assert!(success, "add link should succeed: {}", stderr);

    // 5. cargo check — the generated code MUST compile
    let output = Command::new("cargo")
        .args(["check"])
        .current_dir(&project)
        .output()
        .expect("Failed to execute cargo check");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr_cargo = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Generated code should compile!\nstdout: {}\nstderr: {}",
        stdout,
        stderr_cargo
    );
}

// ============================================================================
// this init --websocket tests
// ============================================================================

#[test]
fn test_init_websocket_cargo_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, stderr) =
        run_this(&["init", "ws-proj", "--websocket", "--no-git"], tmp.path());

    assert!(success, "init --websocket should succeed: {}", stderr);

    let cargo_toml = std::fs::read_to_string(tmp.path().join("ws-proj/Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains(r#"features = ["websocket"]"#),
        "Cargo.toml should contain websocket feature, got:\n{}",
        cargo_toml
    );
}

#[test]
fn test_init_websocket_main_rs() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(&["init", "ws-main", "--websocket", "--no-git"], tmp.path());

    assert!(success);

    let main_rs = std::fs::read_to_string(tmp.path().join("ws-main/src/main.rs")).unwrap();
    assert!(
        main_rs.contains("WebSocketExposure"),
        "main.rs should contain WebSocketExposure"
    );
    assert!(
        main_rs.contains("build_host"),
        "main.rs should use build_host() for WebSocket mode"
    );
    assert!(
        main_rs.contains("with_event_bus"),
        "main.rs should configure EventBus for WebSocket"
    );
    assert!(
        main_rs.contains("/ws"),
        "main.rs should mention /ws endpoint"
    );
}

#[test]
fn test_init_without_websocket_unchanged() {
    // Regression: without --websocket, no WS code should appear
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(&["init", "no-ws", "--no-git"], tmp.path());

    assert!(success);

    let cargo_toml = std::fs::read_to_string(tmp.path().join("no-ws/Cargo.toml")).unwrap();
    assert!(
        !cargo_toml.contains("websocket"),
        "Cargo.toml should NOT contain websocket without flag"
    );

    let main_rs = std::fs::read_to_string(tmp.path().join("no-ws/src/main.rs")).unwrap();
    assert!(
        !main_rs.contains("WebSocketExposure"),
        "main.rs should NOT contain WebSocketExposure without flag"
    );
    assert!(
        main_rs.contains(".build()?"),
        "main.rs should use .build() without WebSocket"
    );
}

#[test]
fn test_init_workspace_websocket_combined() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, stderr) = run_this(
        &["init", "combo-ws", "--workspace", "--websocket", "--no-git"],
        tmp.path(),
    );

    assert!(
        success,
        "init --workspace --websocket should succeed: {}",
        stderr
    );

    let ws_dir = tmp.path().join("combo-ws");

    // Cargo.toml should have both websocket feature AND embedded-frontend
    let cargo_toml = std::fs::read_to_string(ws_dir.join("api/Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains(r#"features = ["websocket"]"#),
        "Cargo.toml should contain websocket feature"
    );
    assert!(
        cargo_toml.contains("embedded-frontend"),
        "Cargo.toml should still have embedded-frontend feature"
    );

    // main.rs should have both WebSocketExposure AND attach_frontend
    let main_rs = std::fs::read_to_string(ws_dir.join("api/src/main.rs")).unwrap();
    assert!(
        main_rs.contains("WebSocketExposure"),
        "main.rs should contain WebSocketExposure"
    );
    assert!(
        main_rs.contains("attach_frontend"),
        "main.rs should still contain attach_frontend"
    );
}

#[test]
fn test_info_websocket_enabled() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "ws-info-proj", "--websocket", "--no-git"],
        tmp.path(),
    );
    assert!(success);

    let project = tmp.path().join("ws-info-proj");
    let (success, stdout, _) = run_this(&["info"], &project);

    assert!(success, "info should succeed in websocket project");
    assert!(
        stdout.contains("enabled"),
        "info should show WebSocket: ✓ enabled, got:\n{}",
        stdout
    );
}

#[test]
fn test_info_websocket_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(&["init", "no-ws-info", "--no-git"], tmp.path());
    assert!(success);

    let project = tmp.path().join("no-ws-info");
    let (success, stdout, _) = run_this(&["info"], &project);

    assert!(success, "info should succeed in non-websocket project");
    assert!(
        stdout.contains("disabled"),
        "info should show WebSocket: ✗ disabled, got:\n{}",
        stdout
    );
}

#[test]
fn test_doctor_websocket_healthy() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "ws-doc-proj", "--websocket", "--no-git"],
        tmp.path(),
    );
    assert!(success);

    let project = tmp.path().join("ws-doc-proj");
    let (success, stdout, _) = run_this(&["doctor"], &project);

    assert!(success, "doctor should pass on healthy websocket project");
    assert!(
        stdout.contains("WebSocket"),
        "doctor should check WebSocket, got:\n{}",
        stdout
    );
    // Should be a pass, not a warning
    assert!(
        !stdout.contains("not found in main.rs"),
        "doctor should not warn on healthy websocket project"
    );
}

#[test]
fn test_doctor_websocket_incoherent() {
    let tmp = tempfile::tempdir().unwrap();
    let (success, _, _) = run_this(
        &["init", "ws-bad-proj", "--websocket", "--no-git"],
        tmp.path(),
    );
    assert!(success);

    let project = tmp.path().join("ws-bad-proj");

    // Tamper with main.rs: remove WebSocketExposure to create incoherence
    let main_path = project.join("src/main.rs");
    let main_content = std::fs::read_to_string(&main_path).unwrap();
    let tampered = main_content.replace("WebSocketExposure", "/* removed */");
    std::fs::write(&main_path, tampered).unwrap();

    let (_, stdout, _) = run_this(&["doctor"], &project);

    assert!(
        stdout.contains("WebSocketExposure not found"),
        "doctor should warn about missing WebSocketExposure, got:\n{}",
        stdout
    );
}
