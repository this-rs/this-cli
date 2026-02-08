use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use super::InitArgs;
use crate::templates::TemplateEngine;
use crate::utils::output;

pub fn run(args: InitArgs) -> Result<()> {
    let project_dir = PathBuf::from(&args.path).join(&args.name);

    if project_dir.exists() {
        bail!(
            "Directory '{}' already exists. Choose a different name or remove it first.",
            project_dir.display()
        );
    }

    output::print_banner();
    output::print_step(&format!("Creating new this-rs project: {}", &args.name));

    // Create directory structure
    let dirs = [
        "",
        "src",
        "src/entities",
        "config",
    ];
    for dir in &dirs {
        let path = project_dir.join(dir);
        std::fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    }

    // Render and write templates
    let engine = TemplateEngine::new()?;
    let mut context = tera::Context::new();
    context.insert("project_name", &args.name);
    context.insert("project_name_snake", &args.name.replace('-', "_"));
    context.insert("port", &args.port);

    let files: &[(&str, &str)] = &[
        ("project/Cargo.toml", "Cargo.toml"),
        ("project/main.rs", "src/main.rs"),
        ("project/module.rs", "src/module.rs"),
        ("project/entities_mod.rs", "src/entities/mod.rs"),
        ("project/links.yaml", "config/links.yaml"),
    ];

    for (template_name, output_path) in files {
        let rendered = engine.render(template_name, &context)
            .with_context(|| format!("Failed to render template: {}", template_name))?;
        let file_path = project_dir.join(output_path);
        std::fs::write(&file_path, &rendered)
            .with_context(|| format!("Failed to write: {}", file_path.display()))?;
        output::print_file_created(output_path);
    }

    // Initialize git repository
    if !args.no_git {
        let gitignore_content = "/target\n*.swp\n.env\n.DS_Store\n";
        std::fs::write(project_dir.join(".gitignore"), gitignore_content)?;
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
    }

    output::print_success(&format!("Project '{}' created successfully!", &args.name));
    output::print_next_steps(&[
        &format!("cd {}", &args.name),
        "cargo run",
        &format!(
            "# Server will start on http://127.0.0.1:{}",
            &args.port
        ),
        "# Add entities with: this add entity <name>",
    ]);

    Ok(())
}
