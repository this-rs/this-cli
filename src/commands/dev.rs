use std::io::{BufRead, BufReader};
use std::path::Path;
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

    // 6. Print dev banner
    print_banner(port, &watcher, webapp, args.api_only);

    // 6b. Print contextual usage examples (best-effort, never fails)
    print_usage_examples(port);

    // 7. Setup Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    // 8. Spawn API process
    let mut api_cmd = build_api_command(&watcher, &api_path, port);
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

    // 9. Spawn frontend process (if applicable)
    let mut front_child = None;
    let mut front_stdout_thread = None;
    let mut front_stderr_thread = None;

    if !args.api_only {
        if let Some(webapp_target) = webapp {
            let front_path = workspace_root.join(&webapp_target.path);

            if !front_path.join("package.json").exists() {
                // Kill API before bailing
                let _ = api_child.kill();
                bail!(
                    "No package.json found in {}. Is the webapp target scaffolded?\n\
                     Scaffold it with: cd {} && npm create vite@latest . -- --template react-ts",
                    front_path.display(),
                    front_path.display()
                );
            }

            let mut front_cmd = Command::new("npm");
            front_cmd
                .args(["run", "dev"])
                .current_dir(&front_path)
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
        } else {
            output::print_info(
                "No webapp target configured — running API only. Add one with: this add target webapp",
            );
        }
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
fn print_banner(
    port: u16,
    watcher: &RustWatcher,
    webapp: Option<&config::TargetConfig>,
    api_only: bool,
) {
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
    if !api_only && let Some(webapp_target) = webapp {
        println!(
            "   {} http://localhost:5173  {}",
            "Front:".green().bold(),
            format!("({})", webapp_target.path).dimmed()
        );
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
}
