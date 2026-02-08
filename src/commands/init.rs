use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::InitArgs;
use crate::templates::TemplateEngine;
use crate::utils::file_writer::FileWriter;
use crate::utils::output;

pub fn run(args: InitArgs, writer: &dyn FileWriter) -> Result<()> {
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
    if !args.no_git && !writer.is_dry_run() {
        let gitignore_content = "/target\n*.swp\n.env\n.DS_Store\n";
        writer.write_file(&project_dir.join(".gitignore"), gitignore_content)?;
        output::print_file_created(".gitignore");

        let status = std::process::Command::new("git")
            .arg("init")
            .current_dir(&project_dir)
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
