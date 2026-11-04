//! `this framework` — the pin, as a user sees it.
//!
//! Two verbs, both read-only: `status` says where the project stands, `check`
//! says whether that standing holds, and exits non-zero when it does not so it
//! can gate a CI.

use std::path::Path;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::framework::{
    self, Diagnostic, Level, Lock, PinSpec, Run, error_count, now_ms, read_lock, read_spec,
    short_rev, validate,
};
use crate::utils::{output, project};

#[derive(Parser)]
pub struct FrameworkCommand {
    #[command(subcommand)]
    pub command: FrameworkSubcommand,
}

#[derive(Subcommand)]
pub enum FrameworkSubcommand {
    /// Show which framework this project targets, and when it was resolved
    Status,

    /// Report an inconsistent pin and exit 1 (usable as a CI gate)
    Check,
}

pub fn run(command: FrameworkSubcommand) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = project::find_workspace_root_from(&cwd)
        .or_else(|| project::detect_project_root_from(&cwd).ok())
        .unwrap_or(cwd);

    let spec = read_spec(&root).map_err(|message| anyhow::anyhow!("{message}"))?;
    let lock = read_lock(&root).map_err(|message| anyhow::anyhow!("{message}"))?;
    let run = Run::detect();

    // What the crate manifest says, when the project declares nothing.
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let declared = framework::manifest_dependency(&manifest);

    let diagnostics = match (&spec, &lock) {
        (None, None) => {
            if let FrameworkSubcommand::Status = command {
                report_undeclared(declared.as_deref());
            }
            match declared {
                Some(_) => vec![Diagnostic::warn(
                    "manifest-only-pin",
                    "the framework is pinned by the manifest only: the channel and the intent are not declared, so nothing can move the project forward",
                )],
                None => Vec::new(),
            }
        }
        (None, Some(lock)) => {
            if let FrameworkSubcommand::Status = command {
                report_lock_only(lock, &root);
            }
            vec![Diagnostic::warn(
                "lock-without-spec",
                "a lock exists but this.yaml declares no `this:` section: nothing checks it",
            )]
        }
        (Some(spec), lock) => {
            if let FrameworkSubcommand::Status = command {
                report_pinned(spec, lock.as_ref(), &root);
            }
            validate(spec, lock.as_ref(), now_ms(), run)
        }
    };

    let blocking = match command {
        FrameworkSubcommand::Status => {
            print_diagnostics(&diagnostics);
            false
        }
        FrameworkSubcommand::Check => {
            print_diagnostics(&diagnostics);
            let errors = error_count(&diagnostics);
            if errors > 0 {
                output::print_error(&format!(
                    "{errors} blocking finding(s) in the framework pin"
                ));
                true
            } else {
                output::print_success("framework pin is consistent");
                false
            }
        }
    };

    if blocking {
        std::process::exit(1);
    }
    Ok(())
}

fn report_undeclared(manifest_dependency: Option<&str>) {
    match manifest_dependency {
        Some(line) => row("framework", line),
        None => row("framework", "not declared"),
    }
    output::print_info("add a `this:` section to this.yaml to declare the source and the channel");
}

fn report_lock_only(lock: &Lock, root: &Path) {
    row("framework", &lock.source.describe_hint());
    row("lock", &framework::lock_path(root).display().to_string());
    row("resolved", &lock.resolved_at);
}

fn report_pinned(spec: &PinSpec, lock: Option<&Lock>, root: &Path) {
    match spec.source() {
        Ok(source) => {
            row("framework", &source.describe());
            // The question a user actually asks of a pin: can somebody else
            // rebuild this? A version and a commit can, a local path cannot.
            row(
                "rebuildable",
                if source.is_reproducible() {
                    "yes, on any machine"
                } else {
                    "no (local path, or a channel not resolved to a commit)"
                },
            );
        }
        Err(message) => row("framework", &format!("incomplete ({message})")),
    }

    if let Some(channel) = spec.channel {
        row("channel", channel.as_str());
    }

    let features = framework::normalized_features(&spec.features);
    let features_text = if features.is_empty() {
        "(none)".to_string()
    } else {
        features.join(", ")
    };
    row("features", &features_text);

    match lock {
        None => row("lock", "absent (this source needs no resolution)"),
        Some(lock) => {
            row("lock", &framework::lock_path(root).display().to_string());
            row("resolved", &lock.resolved_at);
            if let Some(age) = lock.age_ms(now_ms()) {
                row("age", &human_age(age));
            }
            if let Ok(source) = lock.source()
                && let Some(rev) = source.rev()
            {
                row("commit", &short_rev(rev));
            }
            if !lock.crates.is_empty() {
                let crates: Vec<String> = lock
                    .crates
                    .iter()
                    .map(|krate| match krate.rev.as_deref() {
                        Some(rev) => format!("{} @ {}", krate.name, short_rev(rev)),
                        None => krate.name.clone(),
                    })
                    .collect();
                row("crates", &crates.join(", "));
            }
        }
    }
}

fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        match diagnostic.level {
            Level::Error => {
                output::print_error(&format!("{}: {}", diagnostic.code, diagnostic.message))
            }
            Level::Warn => {
                output::print_warn(&format!("{}: {}", diagnostic.code, diagnostic.message))
            }
        }
    }
}

fn row(label: &str, value: &str) {
    output::line(format!("{label:<10} {value}"));
}

/// `3 days`, `5 hours`, `12 min` — enough precision to act on.
fn human_age(ms: u64) -> String {
    let days = ms / 86_400_000;
    if days >= 2 {
        return format!("{days} days ago");
    }
    let hours = ms / 3_600_000;
    if hours >= 2 {
        return format!("{hours} hours ago");
    }
    let minutes = ms / 60_000;
    format!("{minutes} min ago")
}
