use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::DevArgs;
use crate::commands::info;
use crate::config::{self, TargetType};
use crate::utils::project;
use crate::utils::{naming, output};

/// Detected Rust watcher tool on the system.
#[derive(Debug, PartialEq)]
enum RustWatcher {
    CargoWatch,
    Watchexec,
    Bacon,
    None,
}

impl RustWatcher {
    fn label(&self) -> &str {
        match self {
            RustWatcher::CargoWatch => "cargo-watch",
            RustWatcher::Watchexec => "watchexec",
            RustWatcher::Bacon => "bacon",
            RustWatcher::None => "none",
        }
    }
}

/// How `this dev` runs the webapp frontend.
#[derive(Debug)]
enum FrontMode {
    /// vite-rs dev loop inside the API process: the API binary serves the
    /// built SPA, watches the frontend sources, rebuilds on change and
    /// exposes `/__dev` (hot reload + AI bridge). No Node dev server.
    ViteRs {
        front_path: PathBuf,
        build_cmd: String,
        dev_log: PathBuf,
    },
    /// Workspace scaffolded against a published this-rs (no `frontend`
    /// feature): keep the historical Node/Vite dev server child process.
    Legacy { front_path: PathBuf },
    /// API only (no webapp target, or `--api-only`).
    None,
}

/// Whether the workspace API binary serves the SPA itself (this-rs
/// `frontend` feature, i.e. this::server::frontend). Probes the generated api
/// manifest for the inline `"frontend"` feature entry.
fn supports_framework_frontend(api_path: &Path) -> bool {
    let Ok(manifest) = std::fs::read_to_string(api_path.join("Cargo.toml")) else {
        return false;
    };
    manifest.contains("\"frontend\"")
}

/// Bundler command for the webapp: `build:fast` (vite build only) when the
/// scaffold provides it, else the full `npm run build`.
fn webapp_build_command(front_path: &Path) -> Result<String> {
    let pkg = front_path.join("package.json");
    let content = std::fs::read_to_string(&pkg)
        .with_context(|| format!("Failed to read {}", pkg.display()))?;
    if content.contains("\"build:fast\"") {
        Ok("npm run build:fast".to_string())
    } else {
        Ok("npm run build".to_string())
    }
}

/// A `Command` running `cmd` through the platform shell (POSIX `sh -c`,
/// Windows `cmd /C`).
fn shell_command(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(cmd);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

/// Run a bundler command synchronously in `cwd`, failing fast with the tail
/// of the output so the user sees the real error.
fn run_frontend_build(cmd: &str, cwd: &Path) -> Result<()> {
    output::print_step(&format!(
        "Building frontend ({}) in {}…",
        cmd,
        cwd.display()
    ));
    let out = shell_command(cmd)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to execute `{cmd}`"))?;
    if out.status.success() {
        output::print_success("Frontend build OK");
        return Ok(());
    }
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    let tail: String = combined
        .chars()
        .rev()
        .take(2500)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let mut msg = format!(
        "Frontend build failed ({}) — output tail:\n{}",
        out.status, tail
    );
    if !cwd.join("node_modules").exists() {
        msg.push_str(&format!(
            "\n\nHint: dependencies are not installed — run `npm install` in {}",
            cwd.display()
        ));
    }
    bail!("{msg}")
}

/// Decide how the webapp frontend will run, bailing early when a configured
/// webapp target is not scaffolded.
fn frontend_mode(
    args: &DevArgs,
    webapp: Option<&config::TargetConfig>,
    api_path: &Path,
    workspace_root: &Path,
) -> Result<FrontMode> {
    if args.api_only {
        return Ok(FrontMode::None);
    }
    let Some(webapp_target) = webapp else {
        return Ok(FrontMode::None);
    };
    let front_path = workspace_root.join(&webapp_target.path);
    if !front_path.join("package.json").exists() {
        bail!(
            "No package.json found in {}. Is the webapp target scaffolded?\n\
             Scaffold it with: this add target webapp --framework react",
            front_path.display()
        );
    }
    if supports_framework_frontend(api_path) {
        let build_cmd = webapp_build_command(&front_path)?;
        let dev_log = std::env::temp_dir().join(format!(
            "{}-frontend-dev.log",
            api_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("this-rs")
        ));
        Ok(FrontMode::ViteRs {
            front_path,
            build_cmd,
            dev_log,
        })
    } else {
        output::print_warn(
            "The API was scaffolded against a published this-rs without the `frontend` \
             feature — falling back to the legacy `npm run dev` (Node/Vite) flow.\n\
             Re-scaffold the workspace with --this-path <local this-rs> to get the \
             vite-rs dev flow (single process, hot reload, /__dev AI bridge).",
        );
        Ok(FrontMode::Legacy { front_path })
    }
}

/// Entry point for `this dev`.
pub fn run(args: DevArgs) -> Result<()> {
    // 1. Find workspace root
    let workspace_root = project::find_workspace_root()
        .context("Not a this-rs workspace. Run `this dev` from inside a workspace.")?;

    // 2. Load workspace config
    let ws_config = config::load_workspace_config(&workspace_root.join("this.yaml"))?;

    // 3. Determine port
    let port = args.port.unwrap_or(ws_config.api.port);
    let api_path = workspace_root.join(&ws_config.api.path);

    // 4. Detect webapp target
    let webapp = ws_config
        .targets
        .iter()
        .find(|t| t.target_type == TargetType::Webapp);

    // 5. Detect rust watcher
    let watcher = if args.no_watch {
        RustWatcher::None
    } else {
        detect_rust_watcher()
    };

    // 5b. Frontend serving mode — vite-rs in-process flow when the API binary
    // has the this-rs `frontend` feature, legacy Node/Vite otherwise.
    let front_mode = frontend_mode(&args, webapp, &api_path, &workspace_root)?;

    // 6. Print dev banner
    print_banner(port, &watcher, &front_mode, args.api_only);

    // 6b. Print contextual usage examples (best-effort, never fails)
    print_usage_examples(port);

    // 7. Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    // 8. Frontend build + API process (vite-rs flow: the API serves the SPA
    // and rebuilds it on source change — no Node dev server involved).
    if let FrontMode::ViteRs {
        ref front_path,
        ref build_cmd,
        ..
    } = front_mode
    {
        run_frontend_build(build_cmd, front_path)?;
    }

    let mut api_cmd = build_api_command(&watcher, &api_path, port);
    if let FrontMode::ViteRs {
        front_path,
        build_cmd,
        dev_log,
    } = &front_mode
    {
        api_cmd
            .env("SERVE_FRONTEND", "true")
            .env("FRONTEND_PATH", front_path.join("dist"))
            .env("THIS_DEV", "1")
            .env("THIS_WATCH", front_path.join("src"))
            .env("THIS_REBUILD", build_cmd)
            .env("THIS_REBUILD_CWD", front_path)
            .env("THIS_DEV_LOG", dev_log);
    }
    api_cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut api_child = api_cmd
        .spawn()
        .context("Failed to start API server. Is Rust installed?")?;

    // Stream API output with prefix
    let api_stdout = api_child.stdout.take();
    let api_stderr = api_child.stderr.take();
    let r1 = running.clone();
    let r2 = running.clone();

    let api_stdout_thread = api_stdout.map(|out| {
        std::thread::spawn(move || {
            stream_prefixed(BufReader::new(out), "API", Color::Blue, &r1);
        })
    });

    let api_stderr_thread = api_stderr.map(|err| {
        std::thread::spawn(move || {
            stream_prefixed(BufReader::new(err), "API", Color::Blue, &r2);
        })
    });

    // 9. Legacy frontend process — only for workspaces scaffolded against a
    // published this-rs without the `frontend` feature (the vite-rs flow
    // serves the SPA inside the API process instead, see step 8).
    let mut front_child = None;
    let mut front_stdout_thread = None;
    let mut front_stderr_thread = None;

    if let FrontMode::Legacy { front_path } = &front_mode {
        let mut front_cmd = Command::new("npm");
        front_cmd
            .args(["run", "dev"])
            .current_dir(front_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        match front_cmd.spawn() {
            Ok(mut child) => {
                let out = child.stdout.take();
                let err = child.stderr.take();
                let r3 = running.clone();
                let r4 = running.clone();

                front_stdout_thread = out.map(|o| {
                    std::thread::spawn(move || {
                        stream_prefixed(BufReader::new(o), "FRONT", Color::Green, &r3);
                    })
                });

                front_stderr_thread = err.map(|e| {
                    std::thread::spawn(move || {
                        stream_prefixed(BufReader::new(e), "FRONT", Color::Green, &r4);
                    })
                });

                front_child = Some(child);
            }
            Err(e) => {
                output::print_warn(&format!(
                    "Failed to start frontend dev server: {}. Continuing with API only.",
                    e
                ));
            }
        }
    } else if !args.api_only && webapp.is_none() {
        output::print_info(
            "No webapp target configured — running API only. Add one with: this add target webapp",
        );
    }

    // 10. Wait loop — check children and Ctrl+C
    while running.load(Ordering::SeqCst) {
        // Check if API exited
        match api_child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    output::print_error(&format!("API process exited with: {}", status));
                } else {
                    output::print_info("API process exited normally");
                }
                break;
            }
            Err(e) => {
                output::print_error(&format!("Error checking API process: {}", e));
                break;
            }
            Ok(None) => {}
        }

        // Check if front exited
        if let Some(ref mut fc) = front_child {
            match fc.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        output::print_warn(&format!(
                            "Frontend process exited with: {}. API still running.",
                            status
                        ));
                    }
                    front_child = None;
                }
                Err(e) => {
                    output::print_warn(&format!("Error checking frontend process: {}", e));
                    front_child = None;
                }
                Ok(None) => {}
            }
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    // 11. Cleanup — kill children gracefully
    println!();
    output::print_step("Shutting down...");

    let _ = api_child.kill();
    let _ = api_child.wait();

    if let Some(ref mut fc) = front_child {
        let _ = fc.kill();
        let _ = fc.wait();
    }

    // Wait for output threads to finish
    if let Some(t) = api_stdout_thread {
        let _ = t.join();
    }
    if let Some(t) = api_stderr_thread {
        let _ = t.join();
    }
    if let Some(t) = front_stdout_thread {
        let _ = t.join();
    }
    if let Some(t) = front_stderr_thread {
        let _ = t.join();
    }

    output::print_success("Development servers stopped");
    Ok(())
}

/// Detect the best available Rust watcher tool.
fn detect_rust_watcher() -> RustWatcher {
    let candidates = [
        ("cargo-watch", RustWatcher::CargoWatch),
        ("watchexec", RustWatcher::Watchexec),
        ("bacon", RustWatcher::Bacon),
    ];

    for (cmd, variant) in candidates {
        if Command::new(cmd)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return variant;
        }
    }

    RustWatcher::None
}

/// Build the command to run the API server.
fn build_api_command(watcher: &RustWatcher, api_path: &Path, port: u16) -> Command {
    match watcher {
        RustWatcher::CargoWatch => {
            let mut cmd = Command::new("cargo");
            cmd.args(["watch", "-x", "run", "-w", "src/"])
                .current_dir(api_path)
                .env("PORT", port.to_string());
            cmd
        }
        RustWatcher::Watchexec => {
            let mut cmd = Command::new("watchexec");
            cmd.args(["-r", "-e", "rs", "--", "cargo", "run"])
                .current_dir(api_path)
                .env("PORT", port.to_string());
            cmd
        }
        RustWatcher::Bacon => {
            let mut cmd = Command::new("bacon");
            cmd.arg("run")
                .current_dir(api_path)
                .env("PORT", port.to_string());
            cmd
        }
        RustWatcher::None => {
            let mut cmd = Command::new("cargo");
            cmd.arg("run")
                .current_dir(api_path)
                .env("PORT", port.to_string());
            cmd
        }
    }
}

/// Print the dev server startup banner.
fn print_banner(port: u16, watcher: &RustWatcher, front_mode: &FrontMode, api_only: bool) {
    println!();
    println!("  {}", "🚀 Starting development servers...".bold());
    println!();

    // API line
    let watcher_info = match watcher {
        RustWatcher::None => format!("  {}", "(tip: cargo install cargo-watch)".dimmed()),
        w => format!("  {} {}", "✓".green(), w.label().dimmed()),
    };
    println!(
        "   {}  http://127.0.0.1:{}{}",
        "API:".cyan().bold(),
        port,
        watcher_info
    );

    // Front line
    if !api_only {
        match front_mode {
            FrontMode::ViteRs {
                front_path,
                dev_log,
                ..
            } => {
                println!(
                    "   {} http://127.0.0.1:{}  {}",
                    "Front:".green().bold(),
                    port,
                    format!("(vite-rs dev loop — served by the API process)").dimmed()
                );
                println!("        {} {}", "dev log:".dimmed(), dev_log.display());
                println!(
                    "        {} curl -s -X POST localhost:{}/__dev/command -H 'content-type: application/json' -d '{{\"code\":\"document.title\"}}'",
                    "bridge:".dimmed(),
                    port
                );
                let _ = front_path;
            }
            FrontMode::Legacy { front_path } => {
                println!(
                    "   {} http://localhost:5173  {}",
                    "Front:".green().bold(),
                    format!("({}, Node/Vite dev server)", front_path.display()).dimmed()
                );
            }
            FrontMode::None => {}
        }
    }

    println!();
    println!("   {}", "Press Ctrl+C to stop".dimmed());
    println!();
}

/// Print contextual usage examples based on project introspection.
/// Best-effort: silently skips if introspection fails (e.g. not a this-rs project).
fn print_usage_examples(port: u16) {
    let project_info = match info::collect_info() {
        Ok(info) => info,
        Err(_) => return, // graceful fallback — no examples if introspection fails
    };

    let first_entity_route = project_info
        .entities
        .first()
        .map(|e| naming::pluralize(&e.name));

    println!("   {}", "Examples:".bold());
    println!();

    // REST example — always available
    if let Some(ref route) = first_entity_route {
        println!(
            "   {}  curl http://127.0.0.1:{}/{}",
            "[REST]".cyan(),
            port,
            route
        );
    } else {
        println!(
            "   {}  curl http://127.0.0.1:{}/health",
            "[REST]".cyan(),
            port
        );
    }

    // GraphQL example — only if feature enabled
    if project_info.features.graphql {
        if !project_info.entities.is_empty() {
            let entity_name = &project_info.entities[0].name;
            println!(
                "   {}  curl -X POST http://127.0.0.1:{}/graphql -H 'Content-Type: application/json' \\",
                "[GQL] ".magenta(),
                port
            );
            println!(
                "                 -d '{{\"query\": \"{{ {} {{ id }} }}\"}}'",
                naming::pluralize(entity_name)
            );
        } else {
            println!(
                "   {}  open http://127.0.0.1:{}/graphql/playground",
                "[GQL] ".magenta(),
                port
            );
        }
    }

    // gRPC example — only if feature enabled
    if project_info.features.grpc {
        println!(
            "   {}  grpcurl -plaintext 127.0.0.1:{} list",
            "[gRPC]".yellow(),
            port
        );
    }

    // WebSocket example — only if feature enabled
    if project_info.features.websocket {
        println!(
            "   {}  websocat ws://127.0.0.1:{}/ws",
            "[WS]  ".green(),
            port
        );
    }

    println!();
}

/// Build usage examples as a list of (label, command) pairs — useful for testing.
#[cfg(test)]
fn build_usage_examples(
    port: u16,
    entities: &[info::EntityInfo],
    features: &info::FeatureFlags,
) -> Vec<(String, String)> {
    let mut examples = Vec::new();

    let first_entity_route = entities.first().map(|e| naming::pluralize(&e.name));

    // REST — always
    if let Some(ref route) = first_entity_route {
        examples.push((
            "REST".to_string(),
            format!("curl http://127.0.0.1:{}/{}", port, route),
        ));
    } else {
        examples.push((
            "REST".to_string(),
            format!("curl http://127.0.0.1:{}/health", port),
        ));
    }

    // GraphQL
    if features.graphql {
        if let Some(entity) = entities.first() {
            examples.push((
                "GQL".to_string(),
                format!(
                    "curl -X POST http://127.0.0.1:{}/graphql -H 'Content-Type: application/json' -d '{{\"query\": \"{{ {} {{ id }} }}\"}}' ",
                    port,
                    naming::pluralize(&entity.name)
                ),
            ));
        } else {
            examples.push((
                "GQL".to_string(),
                format!("open http://127.0.0.1:{}/graphql/playground", port),
            ));
        }
    }

    // gRPC
    if features.grpc {
        examples.push((
            "gRPC".to_string(),
            format!("grpcurl -plaintext 127.0.0.1:{} list", port),
        ));
    }

    // WebSocket
    if features.websocket {
        examples.push((
            "WS".to_string(),
            format!("websocat ws://127.0.0.1:{}/ws", port),
        ));
    }

    examples
}

/// Color enum for output prefixing.
#[derive(Clone, Copy)]
enum Color {
    Blue,
    Green,
}

/// Stream lines from a reader, prefixing each with a colored label.
fn stream_prefixed<R: std::io::Read>(
    reader: BufReader<R>,
    label: &str,
    color: Color,
    running: &AtomicBool,
) {
    let colored_label = match color {
        Color::Blue => format!("[{}]", label).blue().bold().to_string(),
        Color::Green => format!("[{}]", label).green().bold().to_string(),
    };

    for line in reader.lines() {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        match line {
            Ok(l) => println!("{} {}", colored_label, l),
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_rust_watcher_returns_variant() {
        // Just verify it doesn't panic — result depends on system
        let _watcher = detect_rust_watcher();
    }

    // ------------------------------------------------------------------------
    // vite-rs frontend mode helpers
    // ------------------------------------------------------------------------

    fn webapp_target(path: &str) -> config::TargetConfig {
        config::TargetConfig {
            target_type: TargetType::Webapp,
            framework: Some("react".to_string()),
            runtime: None,
            path: path.to_string(),
        }
    }

    #[test]
    fn test_supports_framework_frontend_true() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            r#"[dependencies]
this = { package = "this-rs", path = "../../this", features = ["frontend", "in-memory"] }
"#,
        )
        .unwrap();
        assert!(supports_framework_frontend(tmp.path()));
    }

    #[test]
    fn test_supports_framework_frontend_false() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            r#"[dependencies]
this = { package = "this-rs", version = "0.0.9", features = ["in-memory"] }
"#,
        )
        .unwrap();
        assert!(!supports_framework_frontend(tmp.path()));
    }

    #[test]
    fn test_webapp_build_command_prefers_build_fast() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts": {"dev": "vite", "build": "tsc -b && vite build", "build:fast": "vite build"}}"#,
        )
        .unwrap();
        assert_eq!(
            webapp_build_command(tmp.path()).unwrap(),
            "npm run build:fast"
        );
    }

    #[test]
    fn test_webapp_build_command_falls_back_to_build() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("package.json"),
            r#"{"scripts": {"dev": "vite", "build": "tsc -b && vite build"}}"#,
        )
        .unwrap();
        assert_eq!(webapp_build_command(tmp.path()).unwrap(), "npm run build");
    }

    #[test]
    fn test_frontend_mode_vite_rs_when_api_supports_it() {
        let tmp = tempfile::tempdir().unwrap();
        let api = tmp.path().join("api");
        let front = tmp.path().join("front");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(
            api.join("Cargo.toml"),
            r#"this = { package = "this-rs", path = "../../this", features = ["frontend"] }"#,
        )
        .unwrap();
        std::fs::write(front.join("package.json"), "{}").unwrap();

        let args = DevArgs {
            api_only: false,
            no_watch: false,
            port: None,
        };
        match frontend_mode(&args, Some(&webapp_target("front")), &api, tmp.path()).unwrap() {
            FrontMode::ViteRs { front_path, .. } => {
                assert_eq!(front_path, tmp.path().join("front"))
            }
            other => panic!("expected ViteRs mode, got {other:?}"),
        }
    }

    #[test]
    fn test_frontend_mode_legacy_without_api_frontend_feature() {
        let tmp = tempfile::tempdir().unwrap();
        let api = tmp.path().join("api");
        let front = tmp.path().join("front");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(
            api.join("Cargo.toml"),
            r#"this = { package = "this-rs", version = "0.0.9", features = ["in-memory"] }"#,
        )
        .unwrap();
        std::fs::write(front.join("package.json"), "{}").unwrap();

        let args = DevArgs {
            api_only: false,
            no_watch: false,
            port: None,
        };
        match frontend_mode(&args, Some(&webapp_target("front")), &api, tmp.path()).unwrap() {
            FrontMode::Legacy { front_path } => {
                assert_eq!(front_path, tmp.path().join("front"))
            }
            other => panic!("expected Legacy mode, got {other:?}"),
        }
    }

    #[test]
    fn test_frontend_mode_bails_without_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        let api = tmp.path().join("api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(
            api.join("Cargo.toml"),
            r#"this = { features = ["frontend"] }"#,
        )
        .unwrap();

        let args = DevArgs {
            api_only: false,
            no_watch: false,
            port: None,
        };
        let err = frontend_mode(&args, Some(&webapp_target("front")), &api, tmp.path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("No package.json"), "{err}");
    }

    #[test]
    fn test_frontend_mode_api_only_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let args = DevArgs {
            api_only: true,
            no_watch: false,
            port: None,
        };
        let mode =
            frontend_mode(&args, Some(&webapp_target("front")), tmp.path(), tmp.path()).unwrap();
        assert!(matches!(mode, FrontMode::None));
    }

    #[test]
    fn test_build_api_command_no_watcher() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::None, tmp.path(), 3000);
        let program = format!("{:?}", cmd.get_program());
        assert!(program.contains("cargo"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, &["run"]);
    }

    #[test]
    fn test_build_api_command_cargo_watch() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::CargoWatch, tmp.path(), 4000);
        let program = format!("{:?}", cmd.get_program());
        assert!(program.contains("cargo"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, &["watch", "-x", "run", "-w", "src/"]);
    }

    #[test]
    fn test_build_api_command_watchexec() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::Watchexec, tmp.path(), 5000);
        let program = format!("{:?}", cmd.get_program());
        assert!(program.contains("watchexec"));
    }

    #[test]
    fn test_build_api_command_bacon() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::Bacon, tmp.path(), 5000);
        let program = format!("{:?}", cmd.get_program());
        assert!(program.contains("bacon"));
    }

    #[test]
    fn test_rust_watcher_labels() {
        assert_eq!(RustWatcher::CargoWatch.label(), "cargo-watch");
        assert_eq!(RustWatcher::Watchexec.label(), "watchexec");
        assert_eq!(RustWatcher::Bacon.label(), "bacon");
        assert_eq!(RustWatcher::None.label(), "none");
    }

    fn make_entity(name: &str) -> info::EntityInfo {
        info::EntityInfo {
            name: name.to_string(),
            fields: vec![],
            is_validated: false,
        }
    }

    #[test]
    fn test_build_usage_examples_all_features() {
        let entities = vec![make_entity("order"), make_entity("invoice")];
        let features = info::FeatureFlags {
            graphql: true,
            websocket: true,
            grpc: true,
        };

        let examples = build_usage_examples(4242, &entities, &features);

        assert_eq!(examples.len(), 4);
        assert_eq!(examples[0].0, "REST");
        assert!(examples[0].1.contains("/orders"));
        assert_eq!(examples[1].0, "GQL");
        assert!(examples[1].1.contains("/graphql"));
        assert!(examples[1].1.contains("orders"));
        assert_eq!(examples[2].0, "gRPC");
        assert!(examples[2].1.contains("grpcurl"));
        assert_eq!(examples[3].0, "WS");
        assert!(examples[3].1.contains("ws://"));
    }

    #[test]
    fn test_build_usage_examples_rest_only() {
        let entities = vec![make_entity("product")];
        let features = info::FeatureFlags {
            graphql: false,
            websocket: false,
            grpc: false,
        };

        let examples = build_usage_examples(3000, &entities, &features);

        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].0, "REST");
        assert!(examples[0].1.contains("/products"));
        assert!(examples[0].1.contains("3000"));
    }

    #[test]
    fn test_build_usage_examples_no_entities_fallback() {
        let entities: Vec<info::EntityInfo> = vec![];
        let features = info::FeatureFlags {
            graphql: true,
            websocket: false,
            grpc: false,
        };

        let examples = build_usage_examples(8080, &entities, &features);

        assert_eq!(examples.len(), 2);
        // REST fallback to /health
        assert!(examples[0].1.contains("/health"));
        // GraphQL fallback to playground
        assert!(examples[1].1.contains("/graphql/playground"));
    }

    #[test]
    fn test_build_usage_examples_uses_correct_port() {
        let entities = vec![make_entity("store")];
        let features = info::FeatureFlags {
            graphql: false,
            websocket: true,
            grpc: true,
        };

        let examples = build_usage_examples(9999, &entities, &features);

        for (_, cmd) in &examples {
            assert!(cmd.contains("9999"), "Port 9999 missing in: {}", cmd);
        }
    }

    // ========================================================================
    // Additional build_api_command tests
    // ========================================================================

    #[test]
    fn test_build_api_command_no_watcher_sets_port_env() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::None, tmp.path(), 7777);
        let envs: Vec<_> = cmd.get_envs().collect();
        let port_env = envs
            .iter()
            .find(|(k, _)| *k == "PORT")
            .expect("PORT env should be set");
        assert_eq!(port_env.1, Some(std::ffi::OsStr::new("7777")));
    }

    #[test]
    fn test_build_api_command_cargo_watch_sets_port_env() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::CargoWatch, tmp.path(), 8080);
        let envs: Vec<_> = cmd.get_envs().collect();
        let port_env = envs
            .iter()
            .find(|(k, _)| *k == "PORT")
            .expect("PORT env should be set");
        assert_eq!(port_env.1, Some(std::ffi::OsStr::new("8080")));
    }

    #[test]
    fn test_build_api_command_watchexec_sets_port_env() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::Watchexec, tmp.path(), 4444);
        let envs: Vec<_> = cmd.get_envs().collect();
        let port_env = envs
            .iter()
            .find(|(k, _)| *k == "PORT")
            .expect("PORT env should be set");
        assert_eq!(port_env.1, Some(std::ffi::OsStr::new("4444")));
    }

    #[test]
    fn test_build_api_command_bacon_sets_port_env() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::Bacon, tmp.path(), 6000);
        let envs: Vec<_> = cmd.get_envs().collect();
        let port_env = envs
            .iter()
            .find(|(k, _)| *k == "PORT")
            .expect("PORT env should be set");
        assert_eq!(port_env.1, Some(std::ffi::OsStr::new("6000")));
    }

    #[test]
    fn test_build_api_command_no_watcher_sets_current_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let api_path = tmp.path().join("my-api");
        std::fs::create_dir_all(&api_path).unwrap();
        let cmd = build_api_command(&RustWatcher::None, &api_path, 3000);
        assert_eq!(cmd.get_current_dir(), Some(api_path.as_path()));
    }

    #[test]
    fn test_build_api_command_cargo_watch_sets_current_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let api_path = tmp.path().join("api");
        std::fs::create_dir_all(&api_path).unwrap();
        let cmd = build_api_command(&RustWatcher::CargoWatch, &api_path, 3000);
        assert_eq!(cmd.get_current_dir(), Some(api_path.as_path()));
    }

    #[test]
    fn test_build_api_command_watchexec_args() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::Watchexec, tmp.path(), 3000);
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, &["-r", "-e", "rs", "--", "cargo", "run"]);
    }

    #[test]
    fn test_build_api_command_bacon_args() {
        let tmp = tempfile::tempdir().unwrap();
        let cmd = build_api_command(&RustWatcher::Bacon, tmp.path(), 3000);
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args, &["run"]);
    }

    #[test]
    fn test_build_api_command_different_ports() {
        let tmp = tempfile::tempdir().unwrap();
        for port in [80, 443, 3000, 8080, 9090, 65535] {
            let cmd = build_api_command(&RustWatcher::None, tmp.path(), port);
            let envs: Vec<_> = cmd.get_envs().collect();
            let port_env = envs
                .iter()
                .find(|(k, _)| *k == "PORT")
                .expect("PORT env should be set");
            assert_eq!(
                port_env.1,
                Some(std::ffi::OsStr::new(&port.to_string())),
                "Port {} should be set in env",
                port
            );
        }
    }

    // ========================================================================
    // RustWatcher variant tests
    // ========================================================================

    #[test]
    fn test_rust_watcher_debug() {
        // Ensure Debug is derived correctly
        let watcher = RustWatcher::CargoWatch;
        let debug_str = format!("{:?}", watcher);
        assert_eq!(debug_str, "CargoWatch");
    }

    #[test]
    fn test_rust_watcher_partial_eq() {
        assert_eq!(RustWatcher::CargoWatch, RustWatcher::CargoWatch);
        assert_eq!(RustWatcher::Watchexec, RustWatcher::Watchexec);
        assert_eq!(RustWatcher::Bacon, RustWatcher::Bacon);
        assert_eq!(RustWatcher::None, RustWatcher::None);
        assert_ne!(RustWatcher::CargoWatch, RustWatcher::None);
        assert_ne!(RustWatcher::Watchexec, RustWatcher::Bacon);
    }

    // ========================================================================
    // Additional build_usage_examples tests
    // ========================================================================

    #[test]
    fn test_build_usage_examples_no_entities_no_features() {
        let entities: Vec<info::EntityInfo> = vec![];
        let features = info::FeatureFlags {
            graphql: false,
            websocket: false,
            grpc: false,
        };

        let examples = build_usage_examples(3000, &entities, &features);

        // Should have only REST with /health fallback
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].0, "REST");
        assert!(examples[0].1.contains("/health"));
        assert!(!examples[0].1.contains("/graphql"));
    }

    #[test]
    fn test_build_usage_examples_websocket_only() {
        let entities = vec![make_entity("user")];
        let features = info::FeatureFlags {
            graphql: false,
            websocket: true,
            grpc: false,
        };

        let examples = build_usage_examples(3000, &entities, &features);

        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].0, "REST");
        assert!(examples[0].1.contains("/users"));
        assert_eq!(examples[1].0, "WS");
        assert!(examples[1].1.contains("websocat"));
        assert!(examples[1].1.contains("ws://"));
        assert!(examples[1].1.contains("/ws"));
    }

    #[test]
    fn test_build_usage_examples_grpc_only() {
        let entities = vec![make_entity("item")];
        let features = info::FeatureFlags {
            graphql: false,
            websocket: false,
            grpc: true,
        };

        let examples = build_usage_examples(5050, &entities, &features);

        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].0, "REST");
        assert!(examples[0].1.contains("/items"));
        assert_eq!(examples[1].0, "gRPC");
        assert!(examples[1].1.contains("grpcurl"));
        assert!(examples[1].1.contains("-plaintext"));
        assert!(examples[1].1.contains("5050"));
        assert!(examples[1].1.contains("list"));
    }

    #[test]
    fn test_build_usage_examples_graphql_only_with_entity() {
        let entities = vec![make_entity("category")];
        let features = info::FeatureFlags {
            graphql: true,
            websocket: false,
            grpc: false,
        };

        let examples = build_usage_examples(4000, &entities, &features);

        assert_eq!(examples.len(), 2);
        assert_eq!(examples[0].0, "REST");
        assert!(examples[0].1.contains("/categories"));
        assert_eq!(examples[1].0, "GQL");
        assert!(examples[1].1.contains("categories"));
        assert!(examples[1].1.contains("Content-Type: application/json"));
        assert!(examples[1].1.contains("4000"));
    }

    #[test]
    fn test_build_usage_examples_uses_first_entity_only() {
        // Only the first entity should be used for route examples
        let entities = vec![
            make_entity("order"),
            make_entity("product"),
            make_entity("user"),
        ];
        let features = info::FeatureFlags {
            graphql: true,
            websocket: false,
            grpc: false,
        };

        let examples = build_usage_examples(3000, &entities, &features);

        // REST should use "orders" (pluralized first entity)
        assert!(examples[0].1.contains("/orders"));
        assert!(!examples[0].1.contains("/products"));
        assert!(!examples[0].1.contains("/users"));

        // GQL should also use "orders"
        assert!(examples[1].1.contains("orders"));
        assert!(!examples[1].1.contains("products"));
    }

    #[test]
    fn test_build_usage_examples_entity_pluralization() {
        // Test with entities that have non-trivial pluralization
        let entities = vec![make_entity("category")];
        let features = info::FeatureFlags {
            graphql: false,
            websocket: false,
            grpc: false,
        };

        let examples = build_usage_examples(3000, &entities, &features);
        assert!(
            examples[0].1.contains("/categories"),
            "Expected 'categories' but got: {}",
            examples[0].1
        );
    }

    #[test]
    fn test_build_usage_examples_no_entities_graphql_grpc_ws() {
        // No entities, but all features enabled → playground fallbacks
        let entities: Vec<info::EntityInfo> = vec![];
        let features = info::FeatureFlags {
            graphql: true,
            websocket: true,
            grpc: true,
        };

        let examples = build_usage_examples(8080, &entities, &features);

        assert_eq!(examples.len(), 4);
        // REST → /health
        assert!(examples[0].1.contains("/health"));
        // GQL → playground
        assert!(examples[1].1.contains("/graphql/playground"));
        // gRPC
        assert!(examples[2].1.contains("grpcurl"));
        // WS
        assert!(examples[3].1.contains("websocat"));
    }

    #[test]
    fn test_build_usage_examples_rest_always_first() {
        // REST should always be the first example regardless of features
        let entities = vec![make_entity("task")];
        let features = info::FeatureFlags {
            graphql: true,
            websocket: true,
            grpc: true,
        };

        let examples = build_usage_examples(3000, &entities, &features);
        assert_eq!(examples[0].0, "REST");
    }

    #[test]
    fn test_build_usage_examples_order_is_rest_gql_grpc_ws() {
        let entities = vec![make_entity("note")];
        let features = info::FeatureFlags {
            graphql: true,
            websocket: true,
            grpc: true,
        };

        let examples = build_usage_examples(3000, &entities, &features);

        assert_eq!(examples.len(), 4);
        assert_eq!(examples[0].0, "REST");
        assert_eq!(examples[1].0, "GQL");
        assert_eq!(examples[2].0, "gRPC");
        assert_eq!(examples[3].0, "WS");
    }

    // ========================================================================
    // detect_rust_watcher tests
    // ========================================================================

    #[test]
    fn test_detect_rust_watcher_result_is_valid_variant() {
        let watcher = detect_rust_watcher();
        // The result must be one of the known variants
        let valid = matches!(
            watcher,
            RustWatcher::CargoWatch
                | RustWatcher::Watchexec
                | RustWatcher::Bacon
                | RustWatcher::None
        );
        assert!(valid, "detect_rust_watcher returned an unexpected variant");
    }

    #[test]
    fn test_detect_rust_watcher_label_not_empty() {
        let watcher = detect_rust_watcher();
        assert!(
            !watcher.label().is_empty(),
            "Watcher label should never be empty"
        );
    }

    // ========================================================================
    // stream_prefixed tests
    // ========================================================================

    #[test]
    fn test_stream_prefixed_reads_all_lines() {
        use std::sync::atomic::AtomicBool;

        let data = b"line 1\nline 2\nline 3\n";
        let reader = BufReader::new(&data[..]);
        let running = AtomicBool::new(true);

        // Should not panic — just prints to stdout
        stream_prefixed(reader, "TEST", Color::Blue, &running);
    }

    #[test]
    fn test_stream_prefixed_stops_on_flag() {
        use std::sync::atomic::AtomicBool;

        let data = b"line 1\nline 2\n";
        let reader = BufReader::new(&data[..]);
        let running = AtomicBool::new(false); // already stopped

        // Should return immediately since running is false
        stream_prefixed(reader, "TEST", Color::Green, &running);
    }

    #[test]
    fn test_stream_prefixed_empty_input() {
        use std::sync::atomic::AtomicBool;

        let data = b"";
        let reader = BufReader::new(&data[..]);
        let running = AtomicBool::new(true);

        stream_prefixed(reader, "EMPTY", Color::Blue, &running);
    }

    // ========================================================================
    // Color enum tests
    // ========================================================================

    #[test]
    fn test_color_clone_copy() {
        let c1 = Color::Blue;
        let c2 = c1; // copy
        let c3 = c1; // still valid since Copy
        assert!(matches!(c2, Color::Blue));
        assert!(matches!(c3, Color::Blue));

        let g1 = Color::Green;
        let g2 = g1;
        assert!(matches!(g2, Color::Green));
    }

    // ========================================================================
    // print_banner smoke tests
    // ========================================================================

    #[test]
    fn test_print_banner_no_watcher_no_webapp() {
        // Should not panic
        print_banner(3000, &RustWatcher::None, &FrontMode::None, false);
    }

    #[test]
    fn test_print_banner_with_watcher_and_webapp() {
        print_banner(
            8080,
            &RustWatcher::CargoWatch,
            &FrontMode::Legacy {
                front_path: "front".into(),
            },
            false,
        );
    }

    #[test]
    fn test_print_banner_vite_rs_flow() {
        print_banner(
            8080,
            &RustWatcher::CargoWatch,
            &FrontMode::ViteRs {
                front_path: "front".into(),
                build_cmd: "npm run build".into(),
                dev_log: "/tmp/x.log".into(),
            },
            false,
        );
    }

    #[test]
    fn test_print_banner_api_only_ignores_webapp() {
        // With api_only=true, the frontend line should be skipped
        print_banner(
            4000,
            &RustWatcher::Watchexec,
            &FrontMode::ViteRs {
                front_path: "front".into(),
                build_cmd: "npm run build".into(),
                dev_log: "/tmp/x.log".into(),
            },
            true,
        );
    }

    #[test]
    fn test_print_banner_all_watchers() {
        let watchers = [
            RustWatcher::CargoWatch,
            RustWatcher::Watchexec,
            RustWatcher::Bacon,
            RustWatcher::None,
        ];
        for watcher in &watchers {
            print_banner(3000, watcher, &FrontMode::None, false);
        }
    }
}
