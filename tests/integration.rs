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
