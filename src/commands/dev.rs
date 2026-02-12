use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::DevArgs;
use crate::config::{self, TargetType};
use crate::utils::output;
use crate::utils::project;

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
}
