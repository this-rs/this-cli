//! `this framework` — the pin, as a user sees it.
//!
//! Two verbs, both read-only: `status` says where the project stands, `check`
//! says whether that standing holds, and exits non-zero when it does not so it
//! can gate a CI.
//!
//! The split matters: [`evaluate`] decides, [`report`] composes what a human
//! reads, and [`run`] only reads files, prints and exits. The first two are
//! pure, so they are tested; the third is a shell thin enough to be obvious.

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

    // What the crate manifest says, when the project declares nothing.
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let declared = framework::manifest_dependency(&manifest);

    let now = now_ms();
    let diagnostics = evaluate(
        spec.as_ref(),
        lock.as_ref(),
        declared.as_deref(),
        now,
        Run::detect(),
    );

    let blocking = match command {
        FrameworkSubcommand::Status => {
            for (label, value) in report(
                spec.as_ref(),
                lock.as_ref(),
                declared.as_deref(),
                &root,
                now,
            ) {
                output::line(format!("{label:<10} {value}"));
            }
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

/// What blocks, and what only deserves a word.
fn evaluate(
    spec: Option<&PinSpec>,
    lock: Option<&Lock>,
    manifest_dependency: Option<&str>,
    now: u64,
    run: Run,
) -> Vec<Diagnostic> {
    match (spec, lock) {
        (None, None) => match manifest_dependency {
            Some(_) => vec![Diagnostic::warn(
                "manifest-only-pin",
                "the framework is pinned by the manifest only: the channel and the intent are not declared, so nothing can move the project forward",
            )],
            None => Vec::new(),
        },
        (None, Some(_)) => vec![Diagnostic::warn(
            "lock-without-spec",
            "a lock exists but this.yaml declares no `this:` section: nothing checks it",
        )],
        (Some(spec), lock) => validate(spec, lock, now, run),
    }
}

/// The lines a user reads, in order: `(label, value)`.
fn report(
    spec: Option<&PinSpec>,
    lock: Option<&Lock>,
    manifest_dependency: Option<&str>,
    root: &Path,
    now: u64,
) -> Vec<(&'static str, String)> {
    let mut rows: Vec<(&'static str, String)> = Vec::new();

    match spec {
        None => {
            rows.push((
                "framework",
                manifest_dependency.map_or_else(|| "not declared".to_string(), str::to_string),
            ));
            rows.push((
                "hint",
                "add a `this:` section to this.yaml to declare the source and the channel"
                    .to_string(),
            ));
        }
        Some(spec) => match spec.source() {
            Ok(source) => {
                rows.push(("framework", source.describe()));
                // The question a user actually asks of a pin: can somebody else
                // rebuild this? A version and a commit can, a local path cannot.
                rows.push((
                    "rebuildable",
                    if source.is_reproducible() {
                        "yes, on any machine".to_string()
                    } else {
                        "no (local path, or a channel not resolved to a commit)".to_string()
                    },
                ));
                if let Some(channel) = spec.channel {
                    rows.push(("channel", channel.as_str().to_string()));
                }
                let features = framework::normalized_features(&spec.features);
                rows.push((
                    "features",
                    if features.is_empty() {
                        "(none)".to_string()
                    } else {
                        features.join(", ")
                    },
                ));
                if lock.is_none() {
                    rows.push((
                        "lock",
                        if source.is_reproducible() {
                            "absent (this source needs no resolution)".to_string()
                        } else {
                            "absent (the channel is not resolved yet)".to_string()
                        },
                    ));
                }
            }
            Err(message) => {
                // Say what the file does contain, not only what it lacks.
                rows.push(("framework", format!("incomplete ({message})")));
                rows.push(("declared", spec.source.describe_hint()));
            }
        },
    }

    if let Some(lock) = lock {
        rows.push(("lock", framework::lock_path(root).display().to_string()));
        rows.push(("resolved", lock.resolved_at.clone()));
        if let Some(age) = lock.age_ms(now) {
            rows.push(("age", human_age(age)));
        }
        if let Ok(source) = lock.source()
            && let Some(rev) = source.rev()
        {
            rows.push(("commit", short_rev(rev)));
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
            rows.push(("crates", crates.join(", ")));
        }
    }

    rows
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::Lock;
    use crate::framework::lock::LockedCrate;
    use crate::framework::pin::{Channel, SourceFields, SourceKind};

    const NOW: u64 = 1_800_000_000_000;

    fn git_spec(rev: Option<&str>, channel: Option<Channel>) -> PinSpec {
        PinSpec {
            source: SourceFields {
                source: SourceKind::Git,
                url: Some("https://github.com/this-rs/this".into()),
                rev: rev.map(str::to_string),
                ..Default::default()
            },
            channel,
            features: vec!["grpc".into()],
        }
    }

    fn git_lock(rev: &str, resolved_at: &str) -> Lock {
        Lock {
            source: SourceFields {
                source: SourceKind::Git,
                url: Some("https://github.com/this-rs/this".into()),
                rev: Some(rev.into()),
                ..Default::default()
            },
            resolved_at: resolved_at.into(),
            channel: Some(Channel::Stable),
            crates: vec![LockedCrate {
                name: "this-rs".into(),
                rev: Some(rev.into()),
            }],
            features: vec!["grpc".into()],
            api_digest: None,
            migrations: Vec::new(),
        }
    }

    fn labels(rows: &[(&'static str, String)]) -> Vec<&'static str> {
        rows.iter().map(|(label, _)| *label).collect()
    }

    fn value<'a>(rows: &'a [(&'static str, String)], label: &str) -> &'a str {
        rows.iter()
            .find(|(l, _)| *l == label)
            .map(|(_, v)| v.as_str())
            .unwrap_or_else(|| panic!("no `{label}` row in {rows:?}"))
    }

    #[test]
    fn a_project_with_nothing_declared_says_so_and_hints() {
        let rows = report(None, None, None, Path::new("/tmp/p"), NOW);
        assert_eq!(value(&rows, "framework"), "not declared");
        assert!(value(&rows, "hint").contains("this:"));
    }

    #[test]
    fn a_manifest_only_pin_is_shown_rather_than_called_absent() {
        let declared = "this = { package = \"this-rs\", version = \"0.0.9\" }";
        let rows = report(None, None, Some(declared), Path::new("/tmp/p"), NOW);
        assert_eq!(value(&rows, "framework"), declared);
    }

    #[test]
    fn an_unresolved_channel_is_not_called_rebuildable() {
        let rows = report(
            Some(&git_spec(None, Some(Channel::Stable))),
            None,
            None,
            Path::new("/tmp/p"),
            NOW,
        );
        assert!(value(&rows, "rebuildable").starts_with("no"));
        assert_eq!(value(&rows, "channel"), "stable");
        assert!(value(&rows, "lock").contains("not resolved"));
    }

    #[test]
    fn a_pinned_commit_is_rebuildable_and_shows_its_lock() {
        let spec = git_spec(Some("abc1234def"), Some(Channel::Stable));
        let lock = git_lock("abc1234def", "2026-11-04T21:59:02Z");
        let rows = report(Some(&spec), Some(&lock), None, Path::new("/tmp/p"), NOW);

        assert!(value(&rows, "rebuildable").starts_with("yes"));
        assert_eq!(value(&rows, "commit"), "abc1234");
        assert_eq!(value(&rows, "crates"), "this-rs @ abc1234");
        assert!(
            value(&rows, "age").ends_with("days ago"),
            "{}",
            value(&rows, "age")
        );
        assert!(value(&rows, "lock").ends_with("framework.lock"));
        assert_eq!(
            labels(&rows),
            vec![
                "framework",
                "rebuildable",
                "channel",
                "features",
                "lock",
                "resolved",
                "age",
                "commit",
                "crates"
            ]
        );
    }

    #[test]
    fn a_crate_without_a_commit_is_named_without_one() {
        let mut lock = git_lock("abc1234", "2026-11-04T21:59:02Z");
        lock.crates.push(LockedCrate {
            name: "this-descriptor".into(),
            rev: None,
        });
        let rows = report(
            Some(&git_spec(Some("abc1234"), None)),
            Some(&lock),
            None,
            Path::new("/tmp/p"),
            NOW,
        );
        assert_eq!(value(&rows, "crates"), "this-rs @ abc1234, this-descriptor");
    }

    #[test]
    fn an_incomplete_declaration_is_reported_as_such() {
        let spec = PinSpec {
            source: SourceFields {
                source: SourceKind::Git,
                ..Default::default()
            },
            ..Default::default()
        };
        let rows = report(Some(&spec), None, None, Path::new("/tmp/p"), NOW);
        assert!(value(&rows, "framework").starts_with("incomplete"));
        assert_eq!(value(&rows, "declared"), "git (url missing)");
    }

    #[test]
    fn no_features_is_said_plainly() {
        let spec = PinSpec::crates_io("0.0.9");
        let rows = report(Some(&spec), None, None, Path::new("/tmp/p"), NOW);
        assert_eq!(value(&rows, "features"), "(none)");
        assert!(value(&rows, "lock").contains("needs no resolution"));
    }

    #[test]
    fn a_manifest_only_pin_warns_but_does_not_block() {
        let found = evaluate(None, None, Some("this = 1"), NOW, Run::Local);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "manifest-only-pin");
        assert!(!found[0].is_error());
    }

    #[test]
    fn a_project_with_nothing_at_all_has_nothing_to_report() {
        assert!(evaluate(None, None, None, NOW, Run::Local).is_empty());
    }

    #[test]
    fn a_lock_without_a_declaration_is_reported() {
        let lock = git_lock("abc1234", "2026-11-04T21:59:02Z");
        let found = evaluate(None, Some(&lock), None, NOW, Run::Local);
        assert_eq!(found[0].code, "lock-without-spec");
    }

    #[test]
    fn a_declaration_is_checked_against_its_lock() {
        let spec = git_spec(Some("abc1234"), None);
        let lock = git_lock("ffffff", "2026-11-04T21:59:02Z");
        let found = evaluate(Some(&spec), Some(&lock), None, NOW, Run::Local);
        assert!(found.iter().any(|d| d.code == "lock-rev-mismatch"));
    }

    #[test]
    fn an_age_is_rounded_to_the_unit_that_speaks() {
        assert_eq!(human_age(3 * 86_400_000), "3 days ago");
        assert_eq!(human_age(5 * 3_600_000), "5 hours ago");
        assert_eq!(human_age(12 * 60_000), "12 min ago");
    }
}
