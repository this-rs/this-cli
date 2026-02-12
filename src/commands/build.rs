use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::BuildArgs;
use crate::config::{self, TargetType, WorkspaceConfig};
use crate::templates::TemplateEngine;
use crate::utils::file_writer::FileWriter;
use crate::utils::output;
use crate::utils::project;

/// Entry point for `this build`.
pub fn run(args: BuildArgs, writer: &dyn FileWriter) -> Result<()> {
    // 1. Find workspace root
    let workspace_root = project::find_workspace_root()
        .context("Not a this-rs workspace. Run `this build` from inside a workspace.")?;

    // 2. Load workspace config
    let config = config::load_workspace_config(&workspace_root.join("this.yaml"))?;

    // 3. Detect webapp target
    let webapp = find_webapp_target(&config);
    let api_path = workspace_root.join(&config.api.path);

    // 4. Dispatch based on flags
    if args.docker {
        require_webapp(&webapp, "--docker")?;
        run_docker(&config, webapp.unwrap(), &workspace_root, writer)?;
    } else if args.embed {
        require_webapp(&webapp, "--embed")?;
        run_embed(&config, webapp.unwrap(), &api_path, &workspace_root)?;
    } else if args.api_only {
        run_api_build(&api_path, args.release)?;
    } else if args.front_only {
        require_webapp(&webapp, "--front-only")?;
        run_front_build(webapp.unwrap(), &workspace_root)?;
    } else {
        // Default mode: build API + front if webapp exists
        run_api_build(&api_path, args.release)?;
        if let Some(webapp) = &webapp {
            run_front_build(webapp, &workspace_root)?;
        } else {
            output::print_info(
                "No webapp target configured — building API only. Add one with: this add target webapp",
            );
        }
    }

    Ok(())
}

/// Find a webapp target in the workspace config.
fn find_webapp_target(config: &WorkspaceConfig) -> Option<&config::TargetConfig> {
    config
        .targets
        .iter()
        .find(|t| t.target_type == TargetType::Webapp)
}

/// Return an error if no webapp target is configured.
fn require_webapp(webapp: &Option<&config::TargetConfig>, flag: &str) -> Result<()> {
    if webapp.is_none() {
        bail!(
            "No webapp target configured. {} requires a webapp target.\n\
             Add one with: this add target webapp",
            flag
        );
    }
    Ok(())
}

/// Build the API with cargo build.
fn run_api_build(api_path: &Path, release: bool) -> Result<()> {
    output::print_step("Building API...");

    let mut cmd = Command::new("cargo");
    cmd.arg("build").current_dir(api_path);
    if release {
        cmd.arg("--release");
    }

    let status = cmd
        .status()
        .context("Failed to execute cargo build. Is Rust installed?")?;

    if !status.success() {
        bail!("cargo build failed with exit code: {}", status);
    }

    if release {
        // Try to display binary size
        if let Some(name) = api_path
            .join("Cargo.toml")
            .exists()
            .then(|| {
                fs::read_to_string(api_path.join("Cargo.toml"))
                    .ok()
                    .and_then(|c| {
                        c.parse::<toml_edit::DocumentMut>()
                            .ok()
                            .and_then(|d| d["package"]["name"].as_str().map(String::from))
                    })
            })
            .flatten()
        {
            let binary_path = api_path.join(format!("target/release/{}", name));
            if let Ok(meta) = fs::metadata(&binary_path) {
                let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
                output::print_info(&format!("Binary size: {:.1} MB", size_mb));
            }
        }
    }

    output::print_success("API build complete");
    Ok(())
}

/// Build the frontend with npm run build.
fn run_front_build(webapp: &config::TargetConfig, workspace_root: &Path) -> Result<()> {
    let front_path = workspace_root.join(&webapp.path);

    if !front_path.join("package.json").exists() {
        bail!(
            "No package.json found in {}. Is the webapp target scaffolded?",
            front_path.display()
        );
    }

    output::print_step("Building frontend...");

    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(&front_path)
        .status()
        .context("Failed to execute npm run build. Is Node.js installed?")?;

    if !status.success() {
        bail!("npm run build failed with exit code: {}", status);
    }

    output::print_success("Frontend build complete");
    Ok(())
}

/// Build embedded binary: npm build → copy dist → cargo build --features embedded-frontend.
fn run_embed(
    config: &WorkspaceConfig,
    webapp: &config::TargetConfig,
    api_path: &Path,
    workspace_root: &Path,
) -> Result<()> {
    // 1. Build frontend
    run_front_build(webapp, workspace_root)?;

    // 2. Copy front dist → api/dist
    let front_dist = workspace_root.join(&webapp.path).join("dist");
    let api_dist = api_path.join("dist");

    if !front_dist.exists() {
        bail!(
            "Frontend build output not found at {}. Did npm run build succeed?",
            front_dist.display()
        );
    }

    output::print_step("Copying frontend assets to API dist/...");
    copy_dir_recursive(&front_dist, &api_dist).context("Failed to copy frontend dist to API")?;

    // 3. Build API with embedded-frontend feature
    output::print_step("Building API with embedded frontend...");

    let status = Command::new("cargo")
        .args(["build", "--release", "--features", "embedded-frontend"])
        .current_dir(api_path)
        .status()
        .context("Failed to execute cargo build --features embedded-frontend")?;

    if !status.success() {
        bail!("cargo build --features embedded-frontend failed");
    }

    // Display binary size
    let binary_path = api_path.join(format!("target/release/{}", config.name));
    if let Ok(meta) = fs::metadata(&binary_path) {
        let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
        println!(
            "  {} Single binary: {} ({:.1} MB)",
            "📦".bold(),
            binary_path.display().to_string().dimmed(),
            size_mb
        );
    }

    output::print_success("Embedded build complete — single binary with frontend");
    Ok(())
}

/// Generate a Dockerfile from template.
fn run_docker(
    config: &WorkspaceConfig,
    webapp: &config::TargetConfig,
    workspace_root: &Path,
    writer: &dyn FileWriter,
) -> Result<()> {
    output::print_step("Generating Dockerfile...");

    let engine = TemplateEngine::new()?;
    let mut context = tera::Context::new();
    context.insert("project_name", &config.name);
    context.insert("api_path", &config.api.path);
    context.insert("webapp_path", &webapp.path);
    context.insert("port", &config.api.port);

    let rendered = engine
        .render("workspace/Dockerfile", &context)
        .context("Failed to render Dockerfile template")?;

    let dockerfile_path = workspace_root.join("Dockerfile");
    writer.write_file(&dockerfile_path, &rendered)?;

    output::print_success("Dockerfile generated");
    output::print_info(&format!("Build with: docker build -t {} .", config.name));

    Ok(())
}

/// Recursively copy a directory's contents to a destination.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_webapp_target_found() {
        let config = WorkspaceConfig {
            name: "test".to_string(),
            api: config::ApiConfig {
                path: "api".to_string(),
                port: 3000,
            },
            targets: vec![config::TargetConfig {
                target_type: TargetType::Webapp,
                framework: Some("react".to_string()),
                runtime: None,
                path: "front".to_string(),
            }],
        };
        assert!(find_webapp_target(&config).is_some());
    }

    #[test]
    fn test_find_webapp_target_not_found() {
        let config = WorkspaceConfig {
            name: "test".to_string(),
            api: config::ApiConfig {
                path: "api".to_string(),
                port: 3000,
            },
            targets: vec![],
        };
        assert!(find_webapp_target(&config).is_none());
    }

    #[test]
    fn test_require_webapp_with_target() {
        let target = config::TargetConfig {
            target_type: TargetType::Webapp,
            framework: Some("react".to_string()),
            runtime: None,
            path: "front".to_string(),
        };
        let result = require_webapp(&Some(&target), "--embed");
        assert!(result.is_ok());
    }

    #[test]
    fn test_require_webapp_without_target() {
        let result = require_webapp(&None, "--embed");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No webapp target configured"));
        assert!(err.contains("--embed"));
        assert!(err.contains("this add target webapp"));
    }

    #[test]
    fn test_copy_dir_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        // Create source structure
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), "hello").unwrap();
        fs::write(src.join("sub/b.txt"), "world").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert!(dst.join("a.txt").exists());
        assert!(dst.join("sub/b.txt").exists());
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "world");
    }
}
