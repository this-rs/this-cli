use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::InitArgs;
use crate::templates::TemplateEngine;
use crate::utils::file_writer::FileWriter;
use crate::utils::output;

/// Entry point: dispatch to classic or workspace mode based on `--workspace` flag.
pub fn run(args: InitArgs, writer: &dyn FileWriter) -> Result<()> {
    if args.workspace {
        run_workspace(args, writer)
    } else {
        run_classic(args, writer)
    }
}

/// Classic init: creates a flat this-rs project (backward-compatible, unchanged behavior).
fn run_classic(args: InitArgs, writer: &dyn FileWriter) -> Result<()> {
    let project_dir = PathBuf::from(&args.path).join(&args.name);

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
fn run_workspace(args: InitArgs, writer: &dyn FileWriter) -> Result<()> {
    let workspace_dir = PathBuf::from(&args.path).join(&args.name);

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

/// Shared git initialization logic for both classic and workspace modes.
fn init_git(args: &InitArgs, project_dir: &PathBuf, writer: &dyn FileWriter) -> Result<()> {
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
