use anyhow::{Result, bail};
use colored::Colorize;

use super::AddTargetArgs;
use crate::config::{self, TargetConfig, TargetType};
use crate::templates::TemplateEngine;
use crate::utils::file_writer::FileWriter;
use crate::utils::output;
use crate::utils::project;

pub fn run(args: AddTargetArgs, writer: &dyn FileWriter) -> Result<()> {
    match args.target_type {
        TargetType::Webapp => run_webapp(args, writer),
        other => bail!(
            "Target type '{}' is not yet supported. Currently only 'webapp' is implemented.",
            other
        ),
    }
}

fn run_webapp(args: AddTargetArgs, writer: &dyn FileWriter) -> Result<()> {
    // 1. Must be inside a workspace
    let workspace_root = project::find_workspace_root().ok_or_else(|| {
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
        let _guard = CwdGuard::new(tmp.path());
        let writer = DryRunWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        let result = run(args, &writer);
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
        let _guard = CwdGuard::new(&ws);
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        let result = run(args, &writer);
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
        let _guard = CwdGuard::new(&ws);
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        run(args, &writer).unwrap();

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
        let _guard = CwdGuard::new(&ws);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // First time: success
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: None,
        };
        run(args, &writer).unwrap();

        // Second time: should fail (target already in this.yaml)
        let args2 = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "vue".to_string(),
            name: Some("front2".to_string()),
        };
        let result = run(args2, &writer);
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
        let _guard = CwdGuard::new(&ws);
        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Webapp,
            framework: "react".to_string(),
            name: Some("frontend".to_string()),
        };
        run(args, &writer).unwrap();

        assert!(ws.join("frontend/package.json").exists());
        assert!(!ws.join("front/package.json").exists());

        let config = config::load_workspace_config(&ws.join("this.yaml")).unwrap();
        assert_eq!(config.targets[0].path, "frontend");
    }

    #[test]
    fn test_add_target_unsupported_type_error() {
        let tmp = TempDir::new().unwrap();
        let ws = setup_workspace(&tmp, "unsupported");
        let _guard = CwdGuard::new(&ws);
        let writer = DryRunWriter::new();
        let args = AddTargetArgs {
            target_type: TargetType::Desktop,
            framework: "react".to_string(),
            name: None,
        };
        let result = run(args, &writer);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not yet supported"),
            "Should mention not supported: {}",
            err
        );
    }

    /// RAII guard for changing CWD in tests
    struct CwdGuard {
        original: std::path::PathBuf,
    }

    impl CwdGuard {
        fn new(path: &std::path::Path) -> Self {
            let original = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { original }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.original);
        }
    }
}
