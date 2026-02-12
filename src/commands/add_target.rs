use anyhow::{Result, bail};
use colored::Colorize;

use super::AddTargetArgs;
use crate::config::{self, TargetConfig, TargetType};
use crate::templates::TemplateEngine;
use crate::utils::file_writer::FileWriter;
use crate::utils::output;
use crate::utils::project;

pub fn run(args: AddTargetArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
    run_in(args, writer, &cwd)
}

/// Run the add target command with an explicit starting directory.
/// This avoids relying on the process-global CWD, making it safe for parallel tests.
pub(crate) fn run_in(
    args: AddTargetArgs,
    writer: &dyn FileWriter,
    cwd: &std::path::Path,
) -> Result<()> {
    match args.target_type {
        TargetType::Webapp => run_webapp(args, writer, cwd),
        TargetType::Desktop => run_desktop(args, writer, cwd),
        TargetType::Ios => run_mobile(args, writer, cwd, "ios"),
        TargetType::Android => run_mobile(args, writer, cwd, "android"),
        other => bail!(
            "Target type '{}' is not yet supported. Currently supported: webapp, desktop, ios, android.",
            other
        ),
    }
}

fn run_webapp(args: AddTargetArgs, writer: &dyn FileWriter, cwd: &std::path::Path) -> Result<()> {
    // 1. Must be inside a workspace
    let workspace_root = project::find_workspace_root_from(cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "Not a this-rs workspace. Run `this init <name> --workspace` first, or cd into a workspace directory."
        )
    })?;

    let this_yaml_path = workspace_root.join("this.yaml");
    let mut config = config::load_workspace_config(&this_yaml_path)?;

    // 2. Check if webapp target already exists
    let already_exists = config
        .targets
        .iter()
        .any(|t| t.target_type == TargetType::Webapp);
    if already_exists {
        bail!(
            "A webapp target already exists in this workspace. Remove it from this.yaml first if you want to recreate it."
        );
    }

    // 3. Determine target directory
    let target_dir_name = args.name.as_deref().unwrap_or("front");
    let target_path = workspace_root.join(target_dir_name);

    if target_path.exists() {
        bail!(
            "Directory '{}' already exists. Remove it or use --name to choose a different name.",
            target_dir_name
        );
    }

    let framework = &args.framework;
    let api_port = config.api.port;

    output::print_step(&format!(
        "Adding webapp target ({}, {})",
        framework, target_dir_name
    ));

    // 4. Create directory structure
    writer.create_dir_all(&target_path)?;
    writer.create_dir_all(&target_path.join("src"))?;
    writer.create_dir_all(&target_path.join("public"))?;

    // 5. Render and write templates
    let engine = TemplateEngine::new()?;
    let mut ctx = tera::Context::new();
    ctx.insert("framework", framework.as_str());
    ctx.insert("api_port", &api_port);
    ctx.insert("project_name", &config.name);

    let templates = [
        ("webapp/package.json", "package.json"),
        ("webapp/vite.config.ts", "vite.config.ts"),
        ("webapp/tsconfig.json", "tsconfig.json"),
        ("webapp/index.html", "index.html"),
        ("webapp/main.tsx", "src/main.tsx"),
        ("webapp/App.tsx", "src/App.tsx"),
        ("webapp/App.css", "src/App.css"),
    ];

    for (template_name, output_file) in &templates {
        let content = engine.render(template_name, &ctx)?;
        let file_path = target_path.join(output_file);
        writer.write_file(&file_path, &content)?;
        output::print_file_created(&format!("{}/{}", target_dir_name, output_file));
    }

    // 6. Update this.yaml
    config.targets.push(TargetConfig {
        target_type: TargetType::Webapp,
        framework: Some(framework.clone()),
        runtime: None,
        path: target_dir_name.to_string(),
    });
    config::save_workspace_config(&this_yaml_path, &config)?;
    output::print_info("Updated this.yaml with webapp target");

    // 7. Print next steps
    println!();
    println!("  {}", "Next steps:".bold());
    println!("    cd {} && npm install", target_dir_name);
    println!("    this dev");
    println!();

    Ok(())
}

fn run_desktop(args: AddTargetArgs, writer: &dyn FileWriter, cwd: &std::path::Path) -> Result<()> {
    // 1. Must be inside a workspace
    let workspace_root = project::find_workspace_root_from(cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "Not a this-rs workspace. Run `this init <name> --workspace` first, or cd into a workspace directory."
        )
    })?;

    let this_yaml_path = workspace_root.join("this.yaml");
    let config_snapshot = config::load_workspace_config(&this_yaml_path)?;

    // 2. Check that a webapp target exists (desktop requires a frontend to wrap)
    let webapp_target = config_snapshot
        .targets
        .iter()
        .find(|t| t.target_type == TargetType::Webapp);
    if webapp_target.is_none() {
        bail!(
            "A webapp target is required before adding a desktop target.\n\
             The desktop app wraps the webapp SPA in a native window.\n\
             Run `this add target webapp` first."
        );
    }
    let front_path = webapp_target.unwrap().path.clone();

    // 3. Check if desktop target already exists
    let already_exists = config_snapshot
        .targets
        .iter()
        .any(|t| t.target_type == TargetType::Desktop);
    if already_exists {
        bail!(
            "A desktop target already exists in this workspace. Remove it from this.yaml first if you want to recreate it."
        );
    }

    // 4. Determine target directory
    let target_dir_name = args.name.as_deref().unwrap_or("targets/desktop");
    let target_path = workspace_root.join(target_dir_name);
    let tauri_path = target_path.join("src-tauri");

    if target_path.exists() {
        bail!(
            "Directory '{}' already exists. Remove it or use --name to choose a different name.",
            target_dir_name
        );
    }

    let project_name = &config_snapshot.name;
    let api_port = config_snapshot.api.port;
    let project_name_snake = crate::utils::naming::to_snake_case(project_name);

    output::print_step(&format!(
        "Adding desktop target (Tauri 2, {})",
        target_dir_name
    ));

    // 5. Create directory structure
    writer.create_dir_all(&tauri_path.join("src"))?;
    writer.create_dir_all(&tauri_path.join("icons"))?;
    writer.create_dir_all(&tauri_path.join("capabilities"))?;

    // 6. Render and write templates
    let engine = TemplateEngine::new()?;
    let mut ctx = tera::Context::new();
    ctx.insert("project_name", project_name);
    ctx.insert("project_name_snake", &project_name_snake);
    ctx.insert("api_port", &api_port);
    ctx.insert("front_path", &front_path);

    let templates = [
        ("desktop/tauri-cargo.toml", "src-tauri/Cargo.toml"),
        ("desktop/tauri.conf.json", "src-tauri/tauri.conf.json"),
        ("desktop/tauri-main.rs", "src-tauri/src/main.rs"),
        ("desktop/tauri-build.rs", "src-tauri/build.rs"),
        (
            "desktop/capabilities.json",
            "src-tauri/capabilities/default.json",
        ),
    ];

    for (template_name, output_file) in &templates {
        let content = engine.render(template_name, &ctx)?;
        let file_path = target_path.join(output_file);
        writer.write_file(&file_path, &content)?;
        output::print_file_created(&format!("{}/{}", target_dir_name, output_file));
    }

    // 7. Update this.yaml
    let mut config = config::load_workspace_config(&this_yaml_path)?;
    config.targets.push(TargetConfig {
        target_type: TargetType::Desktop,
        framework: None,
        runtime: Some("tauri".to_string()),
        path: target_dir_name.to_string(),
    });
    config::save_workspace_config(&this_yaml_path, &config)?;
    output::print_info("Updated this.yaml with desktop target");

    // 8. Print next steps
    println!();
    println!("  {}", "Next steps:".bold());
    println!("    Install Tauri CLI: cargo install tauri-cli@^2");
    println!(
        "    Run desktop app:  cargo tauri dev --manifest-path {}/src-tauri/Cargo.toml",
        target_dir_name
    );
    println!("    Build desktop:    this build --target desktop");
    println!();

    Ok(())
}

fn run_mobile(
    args: AddTargetArgs,
    writer: &dyn FileWriter,
    cwd: &std::path::Path,
    platform: &str,
) -> Result<()> {
    let target_type = match platform {
        "ios" => TargetType::Ios,
        "android" => TargetType::Android,
        _ => bail!("Invalid mobile platform: {}", platform),
    };

    // 1. Must be inside a workspace
    let workspace_root = project::find_workspace_root_from(cwd).ok_or_else(|| {
        anyhow::anyhow!(
            "Not a this-rs workspace. Run `this init <name> --workspace` first, or cd into a workspace directory."
        )
    })?;

    let this_yaml_path = workspace_root.join("this.yaml");
    let config_snapshot = config::load_workspace_config(&this_yaml_path)?;

    // 2. Check that a webapp target exists (mobile wraps the SPA)
    let webapp_target = config_snapshot
        .targets
        .iter()
        .find(|t| t.target_type == TargetType::Webapp);
    if webapp_target.is_none() {
        bail!(
            "A webapp target is required before adding a {} target.\n\
             The {} app wraps the webapp SPA in a native container.\n\
             Run `this add target webapp` first.",
            platform,
            platform
        );
    }
    let front_path = webapp_target.unwrap().path.clone();

    // 3. Check if this mobile target already exists
    let already_exists = config_snapshot
        .targets
        .iter()
        .any(|t| t.target_type == target_type);
    if already_exists {
        bail!(
            "A {} target already exists in this workspace. Remove it from this.yaml first if you want to recreate it.",
            platform
        );
    }

    // 4. Determine target directory
    let default_path = format!("targets/{}", platform);
    let target_dir_name = args.name.as_deref().unwrap_or(&default_path);
    let target_path = workspace_root.join(target_dir_name);

    if target_path.exists() {
        bail!(
            "Directory '{}' already exists. Remove it or use --name to choose a different name.",
            target_dir_name
        );
    }

    let project_name = &config_snapshot.name;
    let api_port = config_snapshot.api.port;

    output::print_step(&format!(
        "Adding {} target (Capacitor, {})",
        platform, target_dir_name
    ));

    // 5. Create directory structure
    writer.create_dir_all(&target_path)?;

    // 6. Render and write templates
    let engine = TemplateEngine::new()?;
    let mut ctx = tera::Context::new();
    ctx.insert("project_name", project_name);
    ctx.insert(
        "project_name_snake",
        &crate::utils::naming::to_snake_case(project_name),
    );
    ctx.insert("api_port", &api_port);
    ctx.insert("front_path", &front_path);
    ctx.insert("platform", platform);

    let templates = [
        ("mobile/capacitor-package.json", "package.json"),
        ("mobile/capacitor.config.ts", "capacitor.config.ts"),
        ("mobile/capacitor-gitignore", ".gitignore"),
    ];

    for (template_name, output_file) in &templates {
        let content = engine.render(template_name, &ctx)?;
        let file_path = target_path.join(output_file);
        writer.write_file(&file_path, &content)?;
        output::print_file_created(&format!("{}/{}", target_dir_name, output_file));
    }

    // 7. Update this.yaml
    let mut config = config::load_workspace_config(&this_yaml_path)?;
    config.targets.push(TargetConfig {
        target_type,
        framework: None,
        runtime: Some("capacitor".to_string()),
        path: target_dir_name.to_string(),
    });
    config::save_workspace_config(&this_yaml_path, &config)?;
    output::print_info(&format!("Updated this.yaml with {} target", platform));

    // 8. Print next steps
    println!();
    println!("  {}", "Next steps:".bold());
    println!("    cd {} && npm install", target_dir_name);
    println!(
        "    npx cap add {}  (initializes the native {} project)",
        platform, platform
    );
    if platform == "ios" {
        println!("    npx cap open ios  (opens Xcode)");
    } else {
        println!("    npx cap open android  (opens Android Studio)");
    }
    println!("    this build --target {}", platform);
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::file_writer::DryRunWriter;
    use tempfile::TempDir;

    /// Create a minimal workspace for testing
    fn setup_workspace(tmp: &TempDir, name: &str) -> std::path::PathBuf {
        let ws = tmp.path().join(name);
        std::fs::create_dir_all(ws.join("api/src")).unwrap();
        let yaml = format!(
            "name: {}\napi:\n  path: api\n  port: 3000\ntargets: []\n",
            name
        );
        std::fs::write(ws.join("this.yaml"), yaml).unwrap();
        ws
    }

    #[test]
    fn test_add_target_args_parsing() {
        use clap::Parser;

        #[derive(Parser)]
        struct Cli {
            #[command(subcommand)]
            cmd: Cmd,
        }
        #[derive(clap::Subcommand)]
        enum Cmd {
            Target(AddTargetArgs),
        }

        let cli = Cli::parse_from(["test", "target", "webapp"]);
        match cli.cmd {
            Cmd::Target(args) => {
                assert_eq!(args.target_type, TargetType::Webapp);
                assert_eq!(args.framework, "react");
                assert!(args.name.is_none());
            }
        }

        let cli = Cli::parse_from(["test", "target", "webapp", "--framework", "vue"]);
        match cli.cmd {
            Cmd::Target(args) => {
                assert_eq!(args.framework, "vue");
            }
        }

        let cli = Cli::parse_from(["test", "target", "webapp", "--name", "frontend"]);
        match cli.cmd {
            Cmd::Target(args) => {
                assert_eq!(args.name, Some("frontend".to_string()));
            }
        }
    }

    #[test]
    fn test_add_target_webapp_outside_workspace_error() {
        let tmp = TempDir::new().unwrap();
        let writer = DryRunWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        // Pass a path that has no this.yaml anywhere up the tree
        let result = run_in(args, &writer, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Not a this-rs workspace"),
            "Error should mention workspace: {}",
            err
        );
    }

    #[test]
    fn test_add_target_webapp_creates_files() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "test_ws");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        // Check files were created
        assert!(ws.join("front/package.json").exists());
        assert!(ws.join("front/vite.config.ts").exists());
        assert!(ws.join("front/tsconfig.json").exists());
        assert!(ws.join("front/index.html").exists());
        assert!(ws.join("front/src/main.tsx").exists());
        assert!(ws.join("front/src/App.tsx").exists());
        assert!(ws.join("front/src/App.css").exists());
    }

    #[test]
    fn test_add_target_webapp_updates_this_yaml() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "yaml_test");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        // Reload and check
        let config = config::load_workspace_config(&ws.join("this.yaml")).unwrap();
        assert_eq!(config.targets.len(), 1);
        assert_eq!(config.targets[0].target_type, TargetType::Webapp);
        assert_eq!(config.targets[0].framework, Some("react".to_string()));
        assert_eq!(config.targets[0].path, "front");
    }

    #[test]
    fn test_add_target_webapp_duplicate_error() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "dup_test");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // First time: success
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        // Second time: should fail (target already in this.yaml)
        let args2 = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "vue".to_string(),
            name: Some("front2".to_string()),
        };
        let result = run_in(args2, &writer, &ws);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention duplicate: {}",
            err
        );
    }

    #[test]
    fn test_add_target_webapp_custom_name() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "custom_name");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: Some("frontend".to_string()),
        };
        run_in(args, &writer, &ws).unwrap();

        assert!(ws.join("frontend/package.json").exists());
        assert!(!ws.join("front/package.json").exists());

        let config = config::load_workspace_config(&ws.join("this.yaml")).unwrap();
        assert_eq!(config.targets[0].path, "frontend");
    }

    #[test]
    fn test_add_target_unsupported_type_error() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "unsupported");
        let writer = DryRunWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Website,
            framework: "react".to_string(),
            name: None,
        };
        let result = run_in(args, &writer, &ws);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not yet supported"),
            "Should mention not supported: {}",
            err
        );
    }

    // ========================================================================
    // Desktop target tests
    // ========================================================================

    /// Create a workspace with a webapp target already added
    fn setup_workspace_with_webapp(tmp: &TempDir, name: &str) -> std::path::PathBuf {
        let ws = tmp.path().join(name);
        std::fs::create_dir_all(ws.join("api/src")).unwrap();
        std::fs::create_dir_all(ws.join("front/src")).unwrap();
        let yaml = format!(
            "name: {}\napi:\n  path: api\n  port: 3000\ntargets:\n  - target_type: webapp\n    framework: react\n    path: front\n",
            name
        );
        std::fs::write(ws.join("this.yaml"), yaml).unwrap();
        ws
    }

    #[test]
    fn test_add_target_desktop_requires_webapp() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "no_webapp");
        let writer = DryRunWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: None,
        };
        let result = run_in(args, &writer, &ws);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("webapp target is required"),
            "Should mention webapp prerequisite: {}",
            err
        );
    }

    #[test]
    fn test_add_target_desktop_creates_files() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "desktop_files");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: None,
        };
        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        // Check Tauri files were created
        assert!(ws.join("targets/desktop/src-tauri/Cargo.toml").exists());
        assert!(
            ws.join("targets/desktop/src-tauri/tauri.conf.json")
                .exists()
        );
        assert!(ws.join("targets/desktop/src-tauri/src/main.rs").exists());
        assert!(ws.join("targets/desktop/src-tauri/build.rs").exists());
        assert!(
            ws.join("targets/desktop/src-tauri/capabilities/default.json")
                .exists()
        );
    }

    #[test]
    fn test_add_target_desktop_updates_this_yaml() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "desktop_yaml");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        let config = config::load_workspace_config(&ws.join("this.yaml")).unwrap();
        assert_eq!(config.targets.len(), 2); // webapp + desktop
        let desktop = config
            .targets
            .iter()
            .find(|t| t.target_type == TargetType::Desktop)
            .expect("Should have desktop target");
        assert_eq!(desktop.runtime, Some("tauri".to_string()));
        assert_eq!(desktop.path, "targets/desktop");
    }

    #[test]
    fn test_add_target_desktop_duplicate_error() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "desktop_dup");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // First time: success
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        // Second time: should fail
        let args2 = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: Some("targets/desktop2".to_string()),
        };
        let result = run_in(args2, &writer, &ws);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention duplicate: {}",
            err
        );
    }

    #[test]
    fn test_add_target_desktop_custom_name() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "desktop_custom");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: Some("my-desktop".to_string()),
        };
        run_in(args, &writer, &ws).unwrap();

        assert!(ws.join("my-desktop/src-tauri/Cargo.toml").exists());
        assert!(!ws.join("targets/desktop/src-tauri/Cargo.toml").exists());

        let config = config::load_workspace_config(&ws.join("this.yaml")).unwrap();
        let desktop = config
            .targets
            .iter()
            .find(|t| t.target_type == TargetType::Desktop)
            .unwrap();
        assert_eq!(desktop.path, "my-desktop");
    }

    #[test]
    fn test_add_target_desktop_tauri_conf_content() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "desktop_conf");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        let conf =
            std::fs::read_to_string(ws.join("targets/desktop/src-tauri/tauri.conf.json")).unwrap();
        assert!(conf.contains("desktop_conf"), "Should contain project name");
        assert!(
            conf.contains("../../../front/dist"),
            "Should point to front dist"
        );
        assert!(
            conf.contains("http://localhost:5173"),
            "Should have dev URL"
        );
    }

    #[test]
    fn test_add_target_desktop_main_rs_content() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "desktop_main");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        let main_rs =
            std::fs::read_to_string(ws.join("targets/desktop/src-tauri/src/main.rs")).unwrap();
        assert!(main_rs.contains("tokio::spawn"), "Should spawn API server");
        assert!(
            main_rs.contains("wait_for_api"),
            "Should wait for health check"
        );
        assert!(main_rs.contains("tauri::Builder"), "Should build Tauri app");
        assert!(
            main_rs.contains("desktop_main"),
            "Should reference project crate"
        );
    }

    // ========================================================================
    // Mobile target tests (iOS & Android)
    // ========================================================================

    #[test]
    fn test_add_target_ios_requires_webapp() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "ios_no_webapp");
        let writer = DryRunWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Ios,
            framework: "react".to_string(),
            name: None,
        };
        let result = run_in(args, &writer, &ws);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("webapp target is required"),
            "Should mention webapp prerequisite: {}",
            err
        );
    }

    #[test]
    fn test_add_target_ios_creates_files() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "ios_files");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Ios,
            framework: "react".to_string(),
            name: None,
        };
        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        assert!(ws.join("targets/ios/package.json").exists());
        assert!(ws.join("targets/ios/capacitor.config.ts").exists());
        assert!(ws.join("targets/ios/.gitignore").exists());
    }

    #[test]
    fn test_add_target_android_creates_files() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "android_files");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Android,
            framework: "react".to_string(),
            name: None,
        };
        let result = run_in(args, &writer, &ws);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        assert!(ws.join("targets/android/package.json").exists());
        assert!(ws.join("targets/android/capacitor.config.ts").exists());
        assert!(ws.join("targets/android/.gitignore").exists());
    }

    #[test]
    fn test_add_target_ios_updates_this_yaml() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "ios_yaml");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Ios,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        let config = config::load_workspace_config(&ws.join("this.yaml")).unwrap();
        assert_eq!(config.targets.len(), 2); // webapp + ios
        let ios = config
            .targets
            .iter()
            .find(|t| t.target_type == TargetType::Ios)
            .expect("Should have ios target");
        assert_eq!(ios.runtime, Some("capacitor".to_string()));
        assert_eq!(ios.path, "targets/ios");
    }

    #[test]
    fn test_add_target_ios_duplicate_error() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "ios_dup");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddTargetArgs {
            target_type: TargetType::Ios,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        let args2 = AddTargetArgs {
            target_type: TargetType::Ios,
            framework: "react".to_string(),
            name: Some("targets/ios2".to_string()),
        };
        let result = run_in(args2, &writer, &ws);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention duplicate: {}",
            err
        );
    }

    #[test]
    fn test_add_target_both_mobile_coexist() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "both_mobile");
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // Add iOS
        let args_ios = AddTargetArgs {
            target_type: TargetType::Ios,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args_ios, &writer, &ws).unwrap();

        // Add Android
        let args_android = AddTargetArgs {
            target_type: TargetType::Android,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args_android, &writer, &ws).unwrap();

        // Both should coexist
        let config = config::load_workspace_config(&ws.join("this.yaml")).unwrap();
        assert_eq!(config.targets.len(), 3); // webapp + ios + android
        assert!(
            config
                .targets
                .iter()
                .any(|t| t.target_type == TargetType::Ios)
        );
        assert!(
            config
                .targets
                .iter()
                .any(|t| t.target_type == TargetType::Android)
        );
        assert!(ws.join("targets/ios/package.json").exists());
        assert!(ws.join("targets/android/package.json").exists());
    }

    #[test]
    fn test_add_target_ios_capacitor_config_content() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "ios_config");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Ios,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        let config_ts =
            std::fs::read_to_string(ws.join("targets/ios/capacitor.config.ts")).unwrap();
        assert!(
            config_ts.contains("com.ios_config.app"),
            "Should have correct appId"
        );
        assert!(
            config_ts.contains("../../front/dist"),
            "Should point to front dist"
        );
        assert!(
            config_ts.contains("http://localhost:3000"),
            "Should have API URL"
        );
        assert!(
            config_ts.contains("CapacitorHttp"),
            "Should enable CapacitorHttp"
        );
    }

    #[test]
    fn test_add_target_android_package_json_content() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace_with_webapp(&tmp, "android_pkg");
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Android,
            framework: "react".to_string(),
            name: None,
        };
        run_in(args, &writer, &ws).unwrap();

        let pkg = std::fs::read_to_string(ws.join("targets/android/package.json")).unwrap();
        assert!(
            pkg.contains("android_pkg-android"),
            "Should have correct name"
        );
        assert!(
            pkg.contains("@capacitor/android"),
            "Should have android platform"
        );
        assert!(pkg.contains("cap sync android"), "Should have sync script");
    }
}
