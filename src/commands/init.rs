use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::InitArgs;
use crate::templates::TemplateEngine;
use crate::utils::file_writer::FileWriter;
use crate::utils::output;

/// Entry point: dispatch to classic or workspace mode based on `--workspace` flag.
pub fn run(args: InitArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
    run_in(args, writer, &cwd)
}

/// Run the init command with an explicit starting directory.
/// This avoids relying on the process-global CWD, making it safe for parallel tests.
pub(crate) fn run_in(args: InitArgs, writer: &dyn FileWriter, cwd: &Path) -> Result<()> {
    if args.workspace {
        run_workspace(args, writer, cwd)
    } else {
        run_classic(args, writer, cwd)
    }
}

/// Classic init: creates a flat this-rs project (backward-compatible, unchanged behavior).
fn run_classic(args: InitArgs, writer: &dyn FileWriter, cwd: &Path) -> Result<()> {
    let project_dir = resolve_project_dir(cwd, &args.path, &args.name);

    if project_dir.exists() && !writer.is_dry_run() {
        bail!(
            "Directory '{}' already exists. Choose a different name or remove it first.",
            project_dir.display()
        );
    }

    if writer.is_dry_run() {
        println!("🔍 {}", "Dry run — no files will be written".cyan().bold());
        println!();
    }

    output::print_banner();
    output::print_step(&format!("Creating new this-rs project: {}", &args.name));

    // Create directory structure
    let dirs = ["", "src", "src/entities", "config"];
    for dir in &dirs {
        let path = project_dir.join(dir);
        writer.create_dir_all(&path)?;
    }

    // Render and write templates
    let engine = TemplateEngine::new()?;
    let mut context = tera::Context::new();
    context.insert("project_name", &args.name);
    context.insert("project_name_snake", &args.name.replace('-', "_"));
    context.insert("port", &args.port);
    if args.websocket {
        context.insert("websocket", &true);
    }
    if args.grpc {
        context.insert("grpc", &true);
    }
    if let Some(ref this_path) = args.this_path {
        context.insert("this_path", this_path);
    }

    let files: &[(&str, &str)] = &[
        ("project/Cargo.toml", "Cargo.toml"),
        ("project/main.rs", "src/main.rs"),
        ("project/module.rs", "src/module.rs"),
        ("project/stores.rs", "src/stores.rs"),
        ("project/entities_mod.rs", "src/entities/mod.rs"),
        ("project/links.yaml", "config/links.yaml"),
    ];

    for (template_name, output_path) in files {
        let rendered = engine
            .render(template_name, &context)
            .with_context(|| format!("Failed to render template: {}", template_name))?;
        let file_path = project_dir.join(output_path);
        writer.write_file(&file_path, &rendered)?;
        if !writer.is_dry_run() {
            output::print_file_created(output_path);
        }
    }

    // Initialize git repository (only in real mode)
    init_git(&args, &project_dir, writer)?;

    if !writer.is_dry_run() {
        output::print_success(&format!("Project '{}' created successfully!", &args.name));
        output::print_next_steps(&[
            &format!("cd {}", &args.name),
            "cargo run",
            &format!("# Server will start on http://127.0.0.1:{}", &args.port),
            "# Add entities with: this add entity <name>",
        ]);
    }

    Ok(())
}

/// Workspace init: creates a multi-target workspace with this.yaml and api/ subdirectory.
fn run_workspace(args: InitArgs, writer: &dyn FileWriter, cwd: &Path) -> Result<()> {
    let workspace_dir = resolve_project_dir(cwd, &args.path, &args.name);

    if workspace_dir.exists() && !writer.is_dry_run() {
        bail!(
            "Directory '{}' already exists. Choose a different name or remove it first.",
            workspace_dir.display()
        );
    }

    if writer.is_dry_run() {
        println!("🔍 {}", "Dry run — no files will be written".cyan().bold());
        println!();
    }

    output::print_banner();
    output::print_step(&format!("Creating new this-rs workspace: {}", &args.name));

    // Create workspace root directory
    writer.create_dir_all(&workspace_dir)?;

    // Generate this.yaml from template
    let engine = TemplateEngine::new()?;
    let mut context = tera::Context::new();
    context.insert("project_name", &args.name);
    context.insert("port", &args.port);

    let this_yaml = engine
        .render("workspace/this.yaml", &context)
        .with_context(|| "Failed to render workspace/this.yaml template")?;
    writer.write_file(&workspace_dir.join("this.yaml"), &this_yaml)?;
    if !writer.is_dry_run() {
        output::print_file_created("this.yaml");
    }

    // Create api/ subdirectory with the classic this-rs scaffold
    let api_dir = workspace_dir.join("api");
    let api_dirs = ["", "src", "src/entities", "config"];
    for dir in &api_dirs {
        writer.create_dir_all(&api_dir.join(dir))?;
    }

    let mut api_context = tera::Context::new();
    api_context.insert("project_name", &args.name);
    api_context.insert("project_name_snake", &args.name.replace('-', "_"));
    api_context.insert("port", &args.port);
    api_context.insert("workspace", &true);
    if args.websocket {
        api_context.insert("websocket", &true);
    }
    if args.grpc {
        api_context.insert("grpc", &true);
    }
    if let Some(ref this_path) = args.this_path {
        api_context.insert("this_path", this_path);
    }

    let api_files: &[(&str, &str)] = &[
        ("project/Cargo.toml", "Cargo.toml"),
        ("project/main.rs", "src/main.rs"),
        ("project/embedded_frontend.rs", "src/embedded_frontend.rs"),
        ("project/module.rs", "src/module.rs"),
        ("project/stores.rs", "src/stores.rs"),
        ("project/entities_mod.rs", "src/entities/mod.rs"),
        ("project/links.yaml", "config/links.yaml"),
    ];

    for (template_name, output_path) in api_files {
        let rendered = engine
            .render(template_name, &api_context)
            .with_context(|| format!("Failed to render template: {}", template_name))?;
        let file_path = api_dir.join(output_path);
        writer.write_file(&file_path, &rendered)?;
        if !writer.is_dry_run() {
            output::print_file_created(&format!("api/{}", output_path));
        }
    }

    // Create api/dist/.gitkeep for future frontend embed
    let dist_dir = api_dir.join("dist");
    writer.create_dir_all(&dist_dir)?;
    writer.write_file(&dist_dir.join(".gitkeep"), "")?;
    if !writer.is_dry_run() {
        output::print_file_created("api/dist/.gitkeep");
    }

    // Initialize git repository at workspace root
    init_git(&args, &workspace_dir, writer)?;

    if !writer.is_dry_run() {
        output::print_success(&format!("Workspace '{}' created successfully!", &args.name));
        output::print_next_steps(&[
            &format!("cd {}", &args.name),
            "cargo run --manifest-path api/Cargo.toml",
            &format!("# Server will start on http://127.0.0.1:{}", &args.port),
            "# Add entities with: this add entity <name>",
            "# Add targets later with: this add target webapp --framework react",
        ]);
    }

    Ok(())
}

/// Resolve the project directory from CWD + args.path + project name.
/// When args.path is "." (default), we resolve relative to cwd.
fn resolve_project_dir(cwd: &Path, path: &str, name: &str) -> PathBuf {
    let base = PathBuf::from(path);
    if base.is_absolute() {
        base.join(name)
    } else {
        cwd.join(base).join(name)
    }
}

/// Shared git initialization logic for both classic and workspace modes.
fn init_git(args: &InitArgs, project_dir: &Path, writer: &dyn FileWriter) -> Result<()> {
    if !args.no_git && !writer.is_dry_run() {
        let gitignore_content = if args.workspace {
            // Workspace .gitignore includes frontend artifacts
            "/target\n*.swp\n.env\n.DS_Store\nnode_modules/\ndist/\n.next/\n.nuxt/\n"
        } else {
            "/target\n*.swp\n.env\n.DS_Store\n"
        };
        writer.write_file(&project_dir.join(".gitignore"), gitignore_content)?;
        output::print_file_created(".gitignore");

        let status = std::process::Command::new("git")
            .arg("init")
            .current_dir(project_dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        match status {
            Ok(s) if s.success() => output::print_info("Initialized git repository"),
            _ => output::print_warn("Could not initialize git repository (git not found?)"),
        }
    } else if !args.no_git && writer.is_dry_run() {
        println!("  {} .gitignore", "Would create:".cyan());
        println!("  {} git init", "Would run:".cyan());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use tempfile::TempDir;

    /// Helper to create default InitArgs for classic mode.
    fn classic_args(name: &str) -> InitArgs {
        InitArgs {
            name: name.to_string(),
            path: ".".to_string(),
            no_git: true,
            port: 3000,
            this_path: None,
            workspace: false,
            websocket: false,
            grpc: false,
        }
    }

    /// Helper to create default InitArgs for workspace mode.
    fn workspace_args(name: &str) -> InitArgs {
        InitArgs {
            name: name.to_string(),
            path: ".".to_string(),
            no_git: true,
            port: 3000,
            this_path: None,
            workspace: true,
            websocket: false,
            grpc: false,
        }
    }

    // ========================================================================
    // Classic mode tests
    // ========================================================================

    #[test]
    fn test_init_classic_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = classic_args("my-project");

        let result = run_in(args, &writer, tmp.path());
        assert!(result.is_ok(), "init should succeed: {:?}", result.err());

        let project = tmp.path().join("my-project");
        assert_file_exists(&project, "Cargo.toml");
        assert_file_exists(&project, "src/main.rs");
        assert_file_exists(&project, "src/module.rs");
        assert_file_exists(&project, "src/stores.rs");
        assert_file_exists(&project, "src/entities/mod.rs");
        assert_file_exists(&project, "config/links.yaml");
        assert_dir_exists(&project, "src");
        assert_dir_exists(&project, "src/entities");
        assert_dir_exists(&project, "config");
    }

    #[test]
    fn test_init_classic_cargo_toml_content() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = classic_args("my-project");

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("my-project");
        assert_file_contains(&project, "Cargo.toml", "this-rs");
        assert_file_contains(&project, "Cargo.toml", "name = \"my-project\"");
        assert_file_contains(&project, "Cargo.toml", "tokio");
        assert_file_contains(&project, "Cargo.toml", "serde");
    }

    #[test]
    fn test_init_classic_main_rs_content() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = classic_args("my-project");

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("my-project");
        assert_file_contains(&project, "src/main.rs", "ServerBuilder");
        assert_file_contains(&project, "src/main.rs", "3000");
        assert_file_contains(&project, "src/main.rs", "#[tokio::main]");
    }

    // ========================================================================
    // Workspace mode tests
    // ========================================================================

    #[test]
    fn test_init_workspace_creates_structure() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = workspace_args("my-workspace");

        let result = run_in(args, &writer, tmp.path());
        assert!(
            result.is_ok(),
            "workspace init should succeed: {:?}",
            result.err()
        );

        let ws = tmp.path().join("my-workspace");
        assert_file_exists(&ws, "this.yaml");
        assert_file_exists(&ws, "api/Cargo.toml");
        assert_file_exists(&ws, "api/src/main.rs");
        assert_file_exists(&ws, "api/src/module.rs");
        assert_file_exists(&ws, "api/src/stores.rs");
        assert_file_exists(&ws, "api/src/entities/mod.rs");
        assert_file_exists(&ws, "api/src/embedded_frontend.rs");
        assert_file_exists(&ws, "api/config/links.yaml");
        assert_file_exists(&ws, "api/dist/.gitkeep");
        assert_dir_exists(&ws, "api");
        assert_dir_exists(&ws, "api/src");
        assert_dir_exists(&ws, "api/src/entities");
        assert_dir_exists(&ws, "api/config");
        assert_dir_exists(&ws, "api/dist");
    }

    #[test]
    fn test_init_workspace_this_yaml_content() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = workspace_args("my-workspace");

        run_in(args, &writer, tmp.path()).unwrap();

        let ws = tmp.path().join("my-workspace");
        assert_file_contains(&ws, "this.yaml", "name: my-workspace");
        assert_file_contains(&ws, "this.yaml", "port: 3000");
        assert_file_contains(&ws, "this.yaml", "path: api");
        assert_file_contains(&ws, "this.yaml", "targets: []");
    }

    #[test]
    fn test_init_workspace_cargo_toml_has_workspace_features() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = workspace_args("my-workspace");

        run_in(args, &writer, tmp.path()).unwrap();

        let ws = tmp.path().join("my-workspace");
        // Workspace Cargo.toml should have embedded-frontend feature
        assert_file_contains(&ws, "api/Cargo.toml", "embedded-frontend");
        assert_file_contains(&ws, "api/Cargo.toml", "rust-embed");
    }

    // ========================================================================
    // WebSocket support
    // ========================================================================

    #[test]
    fn test_init_with_websocket() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = classic_args("ws-project");
        args.websocket = true;

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("ws-project");
        assert_file_contains(&project, "Cargo.toml", "websocket");
        assert_file_contains(&project, "src/main.rs", "WebSocketExposure");
    }

    // ========================================================================
    // gRPC support
    // ========================================================================

    #[test]
    fn test_init_with_grpc() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = classic_args("grpc-project");
        args.grpc = true;

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("grpc-project");
        assert_file_contains(&project, "Cargo.toml", "grpc");
        assert_file_contains(&project, "src/main.rs", "GrpcExposure");
    }

    // ========================================================================
    // Custom port
    // ========================================================================

    #[test]
    fn test_init_with_custom_port() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = classic_args("port-project");
        args.port = 8080;

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("port-project");
        assert_file_contains(&project, "src/main.rs", "8080");
    }

    #[test]
    fn test_init_workspace_with_custom_port() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = workspace_args("port-ws");
        args.port = 9090;

        run_in(args, &writer, tmp.path()).unwrap();

        let ws = tmp.path().join("port-ws");
        assert_file_contains(&ws, "this.yaml", "port: 9090");
        assert_file_contains(&ws, "api/src/main.rs", "9090");
    }

    // ========================================================================
    // Error cases
    // ========================================================================

    #[test]
    fn test_init_directory_already_exists_error() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // Pre-create the target directory
        let existing = tmp.path().join("existing-project");
        std::fs::create_dir_all(&existing).unwrap();

        let args = classic_args("existing-project");
        let result = run_in(args, &writer, tmp.path());

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention 'already exists': {}",
            err
        );
    }

    #[test]
    fn test_init_workspace_directory_already_exists_error() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // Pre-create the target directory
        let existing = tmp.path().join("existing-ws");
        std::fs::create_dir_all(&existing).unwrap();

        let args = workspace_args("existing-ws");
        let result = run_in(args, &writer, tmp.path());

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention 'already exists': {}",
            err
        );
    }

    // ========================================================================
    // no_git flag
    // ========================================================================

    #[test]
    fn test_init_no_git_flag() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = classic_args("nogit-project");
        args.no_git = true;

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("nogit-project");
        // .git should NOT be created since no_git is true
        assert_file_not_exists(&project, ".git");
        // .gitignore should NOT be created since no_git is true
        assert_file_not_exists(&project, ".gitignore");
    }

    #[test]
    fn test_init_with_git_creates_gitignore() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = classic_args("git-project");
        args.no_git = false;

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("git-project");
        // .gitignore should be created when git init is enabled
        assert_file_exists(&project, ".gitignore");
        assert_file_contains(&project, ".gitignore", "/target");
        assert_file_contains(&project, ".gitignore", ".DS_Store");
    }

    #[test]
    fn test_init_workspace_with_git_creates_gitignore() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = workspace_args("git-ws");
        args.no_git = false;

        run_in(args, &writer, tmp.path()).unwrap();

        let ws = tmp.path().join("git-ws");
        assert_file_exists(&ws, ".gitignore");
        assert_file_contains(&ws, ".gitignore", "/target");
        assert_file_contains(&ws, ".gitignore", "node_modules/");
        assert_file_contains(&ws, ".gitignore", "dist/");
    }

    // ========================================================================
    // Combined features
    // ========================================================================

    #[test]
    fn test_init_with_websocket_and_grpc() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = classic_args("combo-project");
        args.websocket = true;
        args.grpc = true;

        run_in(args, &writer, tmp.path()).unwrap();

        let project = tmp.path().join("combo-project");
        assert_file_contains(&project, "Cargo.toml", "websocket");
        assert_file_contains(&project, "Cargo.toml", "grpc");
        assert_file_contains(&project, "src/main.rs", "WebSocketExposure");
        assert_file_contains(&project, "src/main.rs", "GrpcExposure");
    }

    #[test]
    fn test_init_workspace_with_websocket() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = workspace_args("ws-websocket");
        args.websocket = true;

        run_in(args, &writer, tmp.path()).unwrap();

        let ws = tmp.path().join("ws-websocket");
        assert_file_contains(&ws, "api/Cargo.toml", "websocket");
        assert_file_contains(&ws, "api/src/main.rs", "WebSocketExposure");
    }

    #[test]
    fn test_init_workspace_with_grpc() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let mut args = workspace_args("ws-grpc");
        args.grpc = true;

        run_in(args, &writer, tmp.path()).unwrap();

        let ws = tmp.path().join("ws-grpc");
        assert_file_contains(&ws, "api/Cargo.toml", "grpc");
        assert_file_contains(&ws, "api/src/main.rs", "GrpcExposure");
    }

    // ========================================================================
    // resolve_project_dir helper
    // ========================================================================

    #[test]
    fn test_resolve_project_dir_relative_path() {
        let cwd = Path::new("/home/user/projects");
        let dir = resolve_project_dir(cwd, ".", "my-app");
        assert_eq!(dir, PathBuf::from("/home/user/projects/./my-app"));
    }

    #[test]
    fn test_resolve_project_dir_absolute_path() {
        let cwd = Path::new("/home/user/projects");
        let dir = resolve_project_dir(cwd, "/tmp/builds", "my-app");
        assert_eq!(dir, PathBuf::from("/tmp/builds/my-app"));
    }

    #[test]
    fn test_resolve_project_dir_custom_relative() {
        let cwd = Path::new("/home/user/projects");
        let dir = resolve_project_dir(cwd, "subdir", "my-app");
        assert_eq!(dir, PathBuf::from("/home/user/projects/subdir/my-app"));
    }

    // ========================================================================
    // Dry run mode
    // ========================================================================

    #[test]
    fn test_init_classic_dry_run_no_files_created() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::utils::file_writer::DryRunWriter::new();
        let args = classic_args("dry-project");

        let result = run_in(args, &writer, tmp.path());
        assert!(result.is_ok(), "dry run should succeed: {:?}", result.err());

        // In dry-run mode, the project directory should NOT be created
        let project = tmp.path().join("dry-project");
        assert!(
            !project.exists(),
            "Project dir should not exist in dry-run mode"
        );
    }

    #[test]
    fn test_init_workspace_dry_run_no_files_created() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::utils::file_writer::DryRunWriter::new();
        let args = workspace_args("dry-ws");

        let result = run_in(args, &writer, tmp.path());
        assert!(result.is_ok(), "dry run should succeed: {:?}", result.err());

        let ws = tmp.path().join("dry-ws");
        assert!(
            !ws.exists(),
            "Workspace dir should not exist in dry-run mode"
        );
    }

    // ========================================================================
    // Dry run does NOT error on existing directory
    // ========================================================================

    #[test]
    fn test_init_dry_run_allows_existing_directory() {
        let tmp = TempDir::new().unwrap();
        let writer = crate::utils::file_writer::DryRunWriter::new();

        // Pre-create the target directory
        let existing = tmp.path().join("existing-project");
        std::fs::create_dir_all(&existing).unwrap();

        let args = classic_args("existing-project");
        let result = run_in(args, &writer, tmp.path());
        // Dry run should succeed even if directory exists
        assert!(
            result.is_ok(),
            "Dry run should not error on existing directory: {:?}",
            result.err()
        );
    }
}
