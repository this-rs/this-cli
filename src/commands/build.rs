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

    // 3. If --target is specified, dispatch to native target build
    if let Some(ref target_name) = args.target {
        return run_target_build(target_name, &config, &workspace_root);
    }

    // 4. Detect webapp target
    let webapp = find_webapp_target(&config);
    let api_path = workspace_root.join(&config.api.path);

    // 5. Dispatch based on flags
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

/// Dispatch build for a specific native target (desktop, ios, android, or "all").
fn run_target_build(
    target_name: &str,
    config: &WorkspaceConfig,
    workspace_root: &Path,
) -> Result<()> {
    // Find webapp target (needed for frontend build)
    let webapp = find_webapp_target(config);

    if target_name == "all" {
        // Build all native targets (non-webapp)
        let native_targets: Vec<&config::TargetConfig> = config
            .targets
            .iter()
            .filter(|t| {
                matches!(
                    t.target_type,
                    TargetType::Desktop | TargetType::Ios | TargetType::Android
                )
            })
            .collect();

        if native_targets.is_empty() {
            bail!(
                "No native targets configured.\n\
                 Add one with: this add target desktop|ios|android"
            );
        }

        // Build frontend once if any target needs it
        if let Some(webapp) = &webapp {
            run_front_build(webapp, workspace_root)?;
        }

        for target in &native_targets {
            run_single_target_build(target, config, workspace_root, &webapp, false)?;
        }

        output::print_success(&format!(
            "All {} native target(s) built successfully",
            native_targets.len()
        ));
        return Ok(());
    }

    // Match target_name to a configured target
    let target = config
        .targets
        .iter()
        .find(|t| t.target_type.to_string() == target_name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Target '{}' not found in this.yaml. Configured targets: {}",
                target_name,
                config
                    .targets
                    .iter()
                    .map(|t| t.target_type.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    // Ensure it's a native target
    if target.target_type == TargetType::Webapp || target.target_type == TargetType::Website {
        bail!(
            "Target '{}' is not a native target. Use --embed, --front-only, or default build instead.",
            target_name
        );
    }

    // Build frontend first if webapp exists
    if let Some(webapp) = &webapp {
        run_front_build(webapp, workspace_root)?;
    }

    run_single_target_build(target, config, workspace_root, &webapp, true)
}

/// Build a single native target.
fn run_single_target_build(
    target: &config::TargetConfig,
    config: &WorkspaceConfig,
    workspace_root: &Path,
    webapp: &Option<&config::TargetConfig>,
    print_final: bool,
) -> Result<()> {
    match target.target_type {
        TargetType::Desktop => {
            run_build_desktop(target, config, workspace_root)?;
        }
        TargetType::Ios | TargetType::Android => {
            run_build_mobile(target, workspace_root, webapp)?;
        }
        _ => {
            bail!(
                "Target type '{}' does not support native build.",
                target.target_type
            );
        }
    }

    if print_final {
        output::print_success(&format!("{} build complete", target.target_type));
    }

    Ok(())
}

/// Build a desktop target with `cargo tauri build`.
fn run_build_desktop(
    target: &config::TargetConfig,
    config: &WorkspaceConfig,
    workspace_root: &Path,
) -> Result<()> {
    let tauri_dir = workspace_root.join(&target.path).join("src-tauri");

    if !tauri_dir.join("Cargo.toml").exists() {
        bail!(
            "No Cargo.toml found in {}. Is the desktop target scaffolded?\n\
             Run: this add target desktop",
            tauri_dir.display()
        );
    }

    output::print_step(&format!("Building desktop app ({})...", config.name));

    // Use cargo tauri build (requires @tauri-apps/cli or cargo-tauri)
    let status = Command::new("cargo")
        .args(["tauri", "build"])
        .current_dir(&tauri_dir)
        .status()
        .context(
            "Failed to execute 'cargo tauri build'. Is cargo-tauri installed?\n\
             Install with: cargo install tauri-cli",
        )?;

    if !status.success() {
        bail!("cargo tauri build failed with exit code: {}", status);
    }

    Ok(())
}

/// Build a mobile target with `npx cap sync`.
fn run_build_mobile(
    target: &config::TargetConfig,
    workspace_root: &Path,
    webapp: &Option<&config::TargetConfig>,
) -> Result<()> {
    let platform = target.target_type.to_string();
    let target_dir = workspace_root.join(&target.path);

    if !target_dir.join("package.json").exists() {
        bail!(
            "No package.json found in {}. Is the {} target scaffolded?\n\
             Run: this add target {}",
            target_dir.display(),
            platform,
            platform
        );
    }

    // If webapp exists, sync frontend build to Capacitor
    if let Some(webapp) = webapp {
        let front_dist = workspace_root.join(&webapp.path).join("dist");
        if !front_dist.exists() {
            bail!(
                "Frontend build output not found at {}. Build the frontend first.",
                front_dist.display()
            );
        }
    }

    output::print_step(&format!("Syncing {} target...", platform));

    let status = Command::new("npx")
        .args(["cap", "sync", &platform])
        .current_dir(&target_dir)
        .status()
        .context(format!(
            "Failed to execute 'npx cap sync {}'. Is Capacitor installed?",
            platform
        ))?;

    if !status.success() {
        bail!(
            "npx cap sync {} failed with exit code: {}",
            platform,
            status
        );
    }

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

    fn make_config_with_targets(targets: Vec<config::TargetConfig>) -> WorkspaceConfig {
        WorkspaceConfig {
            name: "test".to_string(),
            api: config::ApiConfig {
                path: "api".to_string(),
                port: 3000,
            },
            targets,
        }
    }

    fn webapp_target() -> config::TargetConfig {
        config::TargetConfig {
            target_type: TargetType::Webapp,
            framework: Some("react".to_string()),
            runtime: None,
            path: "front".to_string(),
        }
    }

    fn desktop_target() -> config::TargetConfig {
        config::TargetConfig {
            target_type: TargetType::Desktop,
            framework: None,
            runtime: Some("tauri".to_string()),
            path: "targets/desktop".to_string(),
        }
    }

    fn ios_target() -> config::TargetConfig {
        config::TargetConfig {
            target_type: TargetType::Ios,
            framework: None,
            runtime: Some("capacitor".to_string()),
            path: "targets/ios".to_string(),
        }
    }

    fn android_target() -> config::TargetConfig {
        config::TargetConfig {
            target_type: TargetType::Android,
            framework: None,
            runtime: Some("capacitor".to_string()),
            path: "targets/android".to_string(),
        }
    }

    #[test]
    fn test_run_target_build_unknown_target() {
        let config = make_config_with_targets(vec![webapp_target()]);
        let tmp = tempfile::tempdir().unwrap();
        let result = run_target_build("desktop", &config, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Target 'desktop' not found"));
    }

    #[test]
    fn test_run_target_build_webapp_rejected() {
        let config = make_config_with_targets(vec![webapp_target()]);
        let tmp = tempfile::tempdir().unwrap();
        let result = run_target_build("webapp", &config, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not a native target"));
    }

    #[test]
    fn test_run_target_build_all_no_native() {
        let config = make_config_with_targets(vec![webapp_target()]);
        let tmp = tempfile::tempdir().unwrap();
        let result = run_target_build("all", &config, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No native targets configured"));
    }

    #[test]
    fn test_run_build_desktop_missing_scaffold() {
        let target = desktop_target();
        let config = make_config_with_targets(vec![webapp_target(), desktop_target()]);
        let tmp = tempfile::tempdir().unwrap();
        // No src-tauri dir → should fail
        let result = run_build_desktop(&target, &config, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No Cargo.toml found"));
        assert!(err.contains("this add target desktop"));
    }

    #[test]
    fn test_run_build_mobile_missing_scaffold() {
        let target = ios_target();
        let tmp = tempfile::tempdir().unwrap();
        // No package.json → should fail
        let result = run_build_mobile(&target, tmp.path(), &None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No package.json found"));
        assert!(err.contains("this add target ios"));
    }

    #[test]
    fn test_run_build_mobile_android_missing_scaffold() {
        let target = android_target();
        let tmp = tempfile::tempdir().unwrap();
        let result = run_build_mobile(&target, tmp.path(), &None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No package.json found"));
        assert!(err.contains("this add target android"));
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

    // ========================================================================
    // Additional find_webapp_target tests
    // ========================================================================

    #[test]
    fn test_find_webapp_target_among_multiple_targets() {
        let config = make_config_with_targets(vec![
            desktop_target(),
            webapp_target(),
            ios_target(),
            android_target(),
        ]);
        let result = find_webapp_target(&config);
        assert!(result.is_some());
        assert_eq!(result.unwrap().target_type, TargetType::Webapp);
        assert_eq!(result.unwrap().path, "front");
    }

    #[test]
    fn test_find_webapp_target_only_native_targets() {
        let config =
            make_config_with_targets(vec![desktop_target(), ios_target(), android_target()]);
        assert!(find_webapp_target(&config).is_none());
    }

    #[test]
    fn test_find_webapp_target_returns_first_webapp() {
        // If multiple webapp targets exist, find returns the first one
        let mut second_webapp = webapp_target();
        second_webapp.path = "front2".to_string();
        second_webapp.framework = Some("vue".to_string());

        let config = make_config_with_targets(vec![webapp_target(), second_webapp]);
        let result = find_webapp_target(&config);
        assert!(result.is_some());
        // Should be the first one
        assert_eq!(result.unwrap().path, "front");
        assert_eq!(result.unwrap().framework, Some("react".to_string()));
    }

    #[test]
    fn test_find_webapp_target_website_is_not_webapp() {
        let website = config::TargetConfig {
            target_type: TargetType::Website,
            framework: None,
            runtime: None,
            path: "site".to_string(),
        };
        let config = make_config_with_targets(vec![website]);
        assert!(
            find_webapp_target(&config).is_none(),
            "Website target should not be matched as webapp"
        );
    }

    // ========================================================================
    // Additional require_webapp tests
    // ========================================================================

    #[test]
    fn test_require_webapp_error_includes_flag_name_docker() {
        let result = require_webapp(&None, "--docker");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--docker"));
        assert!(err.contains("No webapp target configured"));
    }

    #[test]
    fn test_require_webapp_error_includes_flag_name_front_only() {
        let result = require_webapp(&None, "--front-only");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("--front-only"));
    }

    #[test]
    fn test_require_webapp_error_suggests_add_target() {
        let result = require_webapp(&None, "--embed");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("this add target webapp"),
            "Error should suggest adding a webapp target"
        );
    }

    // ========================================================================
    // run_target_build error path tests
    // ========================================================================

    #[test]
    fn test_run_target_build_website_rejected() {
        let website = config::TargetConfig {
            target_type: TargetType::Website,
            framework: None,
            runtime: None,
            path: "site".to_string(),
        };
        let config = make_config_with_targets(vec![website]);
        let tmp = tempfile::tempdir().unwrap();
        let result = run_target_build("website", &config, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not a native target"));
    }

    #[test]
    fn test_run_target_build_error_lists_configured_targets() {
        let config =
            make_config_with_targets(vec![webapp_target(), desktop_target(), ios_target()]);
        let tmp = tempfile::tempdir().unwrap();
        let result = run_target_build("android", &config, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Target 'android' not found"));
        assert!(err.contains("webapp"));
        assert!(err.contains("desktop"));
        assert!(err.contains("ios"));
    }

    // ========================================================================
    // run_front_build error path tests
    // ========================================================================

    #[test]
    fn test_run_front_build_missing_package_json() {
        let webapp = webapp_target();
        let tmp = tempfile::tempdir().unwrap();
        // Create the front directory but no package.json
        fs::create_dir_all(tmp.path().join("front")).unwrap();
        let result = run_front_build(&webapp, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No package.json found"));
    }

    #[test]
    fn test_run_front_build_missing_front_dir() {
        let webapp = webapp_target();
        let tmp = tempfile::tempdir().unwrap();
        // Don't even create the front directory
        let result = run_front_build(&webapp, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No package.json found"));
    }

    // ========================================================================
    // run_build_mobile error path tests
    // ========================================================================

    #[test]
    fn test_run_build_mobile_ios_missing_front_dist() {
        let target = ios_target();
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("targets/ios");
        fs::create_dir_all(&target_dir).unwrap();
        // Create package.json so it passes the first check
        fs::write(target_dir.join("package.json"), "{}").unwrap();

        // Provide a webapp that doesn't have dist yet
        let webapp = webapp_target();
        let result = run_build_mobile(&target, tmp.path(), &Some(&webapp));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Frontend build output not found"));
    }

    #[test]
    #[ignore] // Spawns real `npx` process — requires Capacitor CLI installed
    fn test_run_build_mobile_without_webapp_skips_dist_check() {
        // When no webapp is configured, the dist check should be skipped entirely.
        let target = ios_target();
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("targets/ios");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("package.json"), r#"{"scripts":{}}"#).unwrap();

        let result = run_build_mobile(&target, tmp.path(), &None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            !err.contains("Frontend build output not found"),
            "Should not check for frontend dist when no webapp, got: {}",
            err
        );
    }

    // ========================================================================
    // run_build_desktop error path tests
    // ========================================================================

    #[test]
    fn test_run_build_desktop_suggests_add_target_on_missing_scaffold() {
        let target = desktop_target();
        let config = make_config_with_targets(vec![desktop_target()]);
        let tmp = tempfile::tempdir().unwrap();
        // Create the target dir but not src-tauri
        fs::create_dir_all(tmp.path().join("targets/desktop")).unwrap();
        let result = run_build_desktop(&target, &config, tmp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No Cargo.toml found"));
        assert!(err.contains("this add target desktop"));
    }

    // ========================================================================
    // copy_dir_recursive additional tests
    // ========================================================================

    #[test]
    fn test_copy_dir_recursive_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("empty_src");
        let dst = tmp.path().join("empty_dst");
        fs::create_dir_all(&src).unwrap();

        copy_dir_recursive(&src, &dst).unwrap();
        assert!(dst.exists());
        assert!(dst.is_dir());
    }

    #[test]
    fn test_copy_dir_recursive_nested_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        // Create deep nested structure
        fs::create_dir_all(src.join("a/b/c")).unwrap();
        fs::write(src.join("a/b/c/deep.txt"), "deep content").unwrap();
        fs::write(src.join("a/top.txt"), "top content").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert!(dst.join("a/b/c/deep.txt").exists());
        assert!(dst.join("a/top.txt").exists());
        assert_eq!(
            fs::read_to_string(dst.join("a/b/c/deep.txt")).unwrap(),
            "deep content"
        );
        assert_eq!(
            fs::read_to_string(dst.join("a/top.txt")).unwrap(),
            "top content"
        );
    }

    #[test]
    fn test_copy_dir_recursive_overwrites_existing_dst() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");

        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();

        // Pre-populate dst with a file
        fs::write(dst.join("existing.txt"), "old content").unwrap();

        // Copy src (which has a different file)
        fs::write(src.join("new.txt"), "new content").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        // Both files should exist
        assert!(dst.join("existing.txt").exists());
        assert!(dst.join("new.txt").exists());
    }

    // ========================================================================
    // run_docker (Dockerfile template rendering) tests
    // ========================================================================

    #[test]
    fn test_run_docker_generates_dockerfile() {
        let config = make_config_with_targets(vec![webapp_target()]);
        let webapp = webapp_target();
        let tmp = tempfile::tempdir().unwrap();

        let writer = crate::utils::file_writer::RealWriter;
        let result = run_docker(&config, &webapp, tmp.path(), &writer);
        assert!(result.is_ok());

        let dockerfile_path = tmp.path().join("Dockerfile");
        assert!(
            dockerfile_path.exists(),
            "Dockerfile should be generated at workspace root"
        );
    }

    #[test]
    fn test_run_docker_dockerfile_content_has_stages() {
        let config = make_config_with_targets(vec![webapp_target()]);
        let webapp = webapp_target();
        let tmp = tempfile::tempdir().unwrap();

        let writer = crate::utils::file_writer::RealWriter;
        run_docker(&config, &webapp, tmp.path(), &writer).unwrap();

        let content = fs::read_to_string(tmp.path().join("Dockerfile")).unwrap();

        // Stage 1: frontend build
        assert!(
            content.contains("FROM node:"),
            "Dockerfile should have a node stage"
        );
        assert!(
            content.contains("npm ci"),
            "Dockerfile should install npm dependencies"
        );
        assert!(
            content.contains("npm run build"),
            "Dockerfile should build frontend"
        );

        // Stage 2: Rust builder
        assert!(
            content.contains("FROM rust:"),
            "Dockerfile should have a Rust builder stage"
        );
        assert!(
            content.contains("cargo build --release"),
            "Dockerfile should build Rust in release mode"
        );
        assert!(
            content.contains("embedded-frontend"),
            "Dockerfile should use embedded-frontend feature"
        );

        // Stage 3: Runtime
        assert!(
            content.contains("FROM alpine:"),
            "Dockerfile should have an Alpine runtime stage"
        );
    }

    #[test]
    fn test_run_docker_dockerfile_uses_project_name() {
        let mut config = make_config_with_targets(vec![webapp_target()]);
        config.name = "my-cool-app".to_string();
        let webapp = webapp_target();
        let tmp = tempfile::tempdir().unwrap();

        let writer = crate::utils::file_writer::RealWriter;
        run_docker(&config, &webapp, tmp.path(), &writer).unwrap();

        let content = fs::read_to_string(tmp.path().join("Dockerfile")).unwrap();
        assert!(
            content.contains("my-cool-app"),
            "Dockerfile should reference project name"
        );
    }

    #[test]
    fn test_run_docker_dockerfile_uses_correct_paths() {
        let mut config = make_config_with_targets(vec![webapp_target()]);
        config.api.path = "backend".to_string();
        let mut webapp = webapp_target();
        webapp.path = "frontend".to_string();
        let tmp = tempfile::tempdir().unwrap();

        let writer = crate::utils::file_writer::RealWriter;
        run_docker(&config, &webapp, tmp.path(), &writer).unwrap();

        let content = fs::read_to_string(tmp.path().join("Dockerfile")).unwrap();
        assert!(
            content.contains("backend"),
            "Dockerfile should reference api path"
        );
        assert!(
            content.contains("frontend"),
            "Dockerfile should reference webapp path"
        );
    }

    #[test]
    fn test_run_docker_dockerfile_uses_correct_port() {
        let mut config = make_config_with_targets(vec![webapp_target()]);
        config.api.port = 9090;
        let webapp = webapp_target();
        let tmp = tempfile::tempdir().unwrap();

        let writer = crate::utils::file_writer::RealWriter;
        run_docker(&config, &webapp, tmp.path(), &writer).unwrap();

        let content = fs::read_to_string(tmp.path().join("Dockerfile")).unwrap();
        assert!(
            content.contains("EXPOSE 9090"),
            "Dockerfile should EXPOSE the configured port, got: {}",
            content
        );
    }

    #[test]
    fn test_run_docker_dockerfile_has_cmd() {
        let mut config = make_config_with_targets(vec![webapp_target()]);
        config.name = "myapp".to_string();
        let webapp = webapp_target();
        let tmp = tempfile::tempdir().unwrap();

        let writer = crate::utils::file_writer::RealWriter;
        run_docker(&config, &webapp, tmp.path(), &writer).unwrap();

        let content = fs::read_to_string(tmp.path().join("Dockerfile")).unwrap();
        assert!(
            content.contains("CMD [\"myapp\"]"),
            "Dockerfile should have CMD with project name"
        );
    }

    // ========================================================================
    // run_target_build "all" mode tests
    // ========================================================================

    #[test]
    #[ignore] // Spawns real `npm run build` — requires Node.js + npm installed
    fn test_run_target_build_all_with_desktop_fails_at_scaffold() {
        // "all" finds native targets and tries to build them — it should fail
        // at the scaffold check because no src-tauri exists
        let config = make_config_with_targets(vec![webapp_target(), desktop_target()]);
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("front/dist")).unwrap();
        fs::write(tmp.path().join("front/package.json"), "{}").unwrap();
        let result = run_target_build("all", &config, tmp.path());
        assert!(result.is_err());
    }

    // ========================================================================
    // run_api_build error path test
    // ========================================================================

    #[test]
    #[ignore] // Spawns real `cargo build` process
    fn test_run_api_build_non_existent_path() {
        // If the api_path doesn't exist, cargo build should fail
        let tmp = tempfile::tempdir().unwrap();
        let fake_api_path = tmp.path().join("nonexistent-api");
        let result = run_api_build(&fake_api_path, false);
        assert!(
            result.is_err(),
            "Should fail when API path doesn't exist for cargo build"
        );
    }
}
