//! Moving a project from one commit of the framework to another.
//!
//! A version bump is an edit; moving a pin is a **decision with an undo**. So
//! the three files that carry the pin are recomputed in memory first
//! ([`plan`]), written only if the user says so, and the lock remembers the
//! commit it replaced — which is what makes [`Lock::previous_rev`] a rollback
//! rather than a backup file to find later.
//!
//! Nothing here knows about the network: an upgrade names a commit. Resolving a
//! channel to a commit is a different act, and a different failure.

use crate::framework::lock::LockedCrate;
use crate::framework::pin::{Channel, PinSpec, Source};
use crate::framework::{Lock, lock::iso8601_from_ms, spec_yaml};

/// The files an upgrade rewrites, computed before anything is written.
#[derive(Debug)]
pub struct Edits {
    /// The whole `this.yaml`, with the new declaration.
    pub specification: String,
    /// The whole `Cargo.toml`, with the new commit in the `this` dependency.
    pub manifest: String,
    /// The new lock, carrying the commit it replaces.
    pub lock: Lock,
}

/// Compute the three edits of moving `spec` to `target_rev`.
pub fn plan(
    spec_text: &str,
    manifest_text: &str,
    spec: &PinSpec,
    target_rev: &str,
    previous_rev: Option<&str>,
    crates: &[String],
    now_ms: u64,
) -> Result<Edits, String> {
    let current = spec.source()?;
    let url = match &current {
        Source::Git { url, .. } => url.clone(),
        other => {
            return Err(format!(
                "an upgrade moves a git pin; this project is pinned by {}",
                other.describe()
            ));
        }
    };

    let target = Source::Git {
        url,
        rev: Some(target_rev.to_string()),
    };

    let new_spec = PinSpec {
        source: target.to_fields(),
        channel: spec.channel,
        features: spec.features.clone(),
    };

    Ok(Edits {
        specification: edit_spec(spec_text, &new_spec)?,
        manifest: edit_manifest_rev(manifest_text, target_rev)?,
        lock: new_lock(
            &target,
            crates,
            &spec.features,
            previous_rev,
            spec.channel,
            now_ms,
        )?,
    })
}

/// Rewrite the `rev` of the `this` dependency, keeping everything else exactly
/// as written — features, `package`, comments and ordering included.
pub fn edit_manifest_rev(manifest: &str, rev: &str) -> Result<String, String> {
    let mut document = manifest
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| error.to_string())?;

    let dependencies = document
        .get_mut("dependencies")
        .and_then(|section| section.as_table_like_mut())
        .ok_or_else(|| "the manifest has no [dependencies] section".to_string())?;

    let dependency = dependencies
        .get_mut("this")
        .ok_or_else(|| "the manifest has no `this` dependency".to_string())?;

    let table = dependency.as_table_like_mut().ok_or_else(|| {
        "the `this` dependency is not a table: a version string has no commit to move".to_string()
    })?;

    if table.get("git").is_none() {
        return Err("the `this` dependency does not name a repository".to_string());
    }

    table.insert("rev", toml_edit::value(rev));
    Ok(document.to_string())
}

/// Replace the top-level `this:` block of a `this.yaml`, keeping everything
/// around it. A file without one gets it appended.
pub fn edit_spec(text: &str, spec: &PinSpec) -> Result<String, String> {
    let block = spec_yaml(spec)?;

    let mut out = String::new();
    let mut inside = false;
    let mut replaced = false;

    for line in text.lines() {
        let top_level = !line.is_empty() && !line.starts_with([' ', '\t', '#']);

        if !inside && line.trim_end() == "this:" && top_level {
            out.push_str(&block);
            inside = true;
            replaced = true;
            continue;
        }

        // The block ends at the next top-level key.
        if inside && top_level {
            inside = false;
        }

        if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !replaced {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&block);
    }

    Ok(out)
}

/// The lock a move produces: the target commit on every crate, the date, and
/// the commit being replaced.
pub fn new_lock(
    source: &Source,
    crates: &[String],
    features: &[String],
    previous_rev: Option<&str>,
    channel: Option<Channel>,
    now_ms: u64,
) -> Result<Lock, String> {
    let rev = source
        .rev()
        .ok_or_else(|| "a lock needs a commit: a branch is not a pin".to_string())?;

    Ok(Lock {
        source: source.to_fields(),
        resolved_at: iso8601_from_ms(now_ms),
        channel,
        crates: crates
            .iter()
            .map(|name| LockedCrate::new(name, Some(rev.to_string())))
            .collect(),
        // The features the lock was resolved with, not an empty list: otherwise
        // `validate` reports a drift that never happened.
        features: crate::framework::normalized_features(features),
        api_digest: None,
        migrations: Vec::new(),
        previous_rev: previous_rev.map(str::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::lock::iso8601_ms;
    use crate::framework::pin::{SourceFields, SourceKind};
    use crate::framework::pin_from_yaml;

    const NOW: u64 = 1_793_829_542_000; // 2026-11-04T21:59:02Z

    const MANIFEST: &str = r#"[package]
name = "api"
version = "0.1.0"

[dependencies]
this = { package = "this-rs", git = "https://github.com/this-rs/this", rev = "aaaaaaa", features = ["grpc"] }
anyhow = "1"
"#;

    const SPEC: &str = "name: shop\n\napi:\n  path: api\n  port: 3000\n\ntargets: []\n\nthis:\n  source: git\n  url: https://github.com/this-rs/this\n  rev: aaaaaaa\n  channel: stable\n";

    fn git_spec(rev: Option<&str>) -> PinSpec {
        PinSpec {
            source: SourceFields {
                source: SourceKind::Git,
                url: Some("https://github.com/this-rs/this".into()),
                rev: rev.map(str::to_string),
                ..Default::default()
            },
            channel: Some(Channel::Stable),
            features: vec!["grpc".into()],
        }
    }

    #[test]
    fn an_iso_date_is_formatted_and_read_back() {
        assert_eq!(iso8601_from_ms(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_from_ms(NOW), "2026-11-04T21:59:02Z");
        for ms in [0, 1_000, NOW, 1_922_400_000_000] {
            assert_eq!(
                iso8601_ms(&iso8601_from_ms(ms)),
                Some(ms),
                "round trip of {ms}"
            );
        }
    }

    #[test]
    fn the_manifest_keeps_everything_but_the_commit() {
        let edited = edit_manifest_rev(MANIFEST, "bbbbbbb").unwrap();
        assert!(edited.contains(r#"rev = "bbbbbbb""#), "{edited}");
        assert!(edited.contains(r#"features = ["grpc"]"#), "{edited}");
        assert!(edited.contains(r#"package = "this-rs""#), "{edited}");
        assert!(edited.contains(r#"anyhow = "1""#), "{edited}");
        assert!(!edited.contains("aaaaaaa"), "{edited}");
    }

    #[test]
    fn a_version_string_has_no_commit_to_move() {
        let manifest = "[dependencies]\nthis = \"0.0.9\"\n";
        let error = edit_manifest_rev(manifest, "bbbbbbb").unwrap_err();
        assert!(error.contains("version string"), "{error}");
    }

    #[test]
    fn a_manifest_without_the_dependency_is_refused() {
        let error = edit_manifest_rev("[dependencies]\nanyhow = \"1\"\n", "b").unwrap_err();
        assert!(error.contains("no `this` dependency"), "{error}");
    }

    #[test]
    fn the_declaration_is_replaced_and_the_rest_kept() {
        let edited = edit_spec(SPEC, &git_spec(Some("bbbbbbb"))).unwrap();

        assert!(edited.contains("name: shop"), "{edited}");
        assert!(edited.contains("path: api"), "{edited}");
        assert!(edited.contains("targets: []"), "{edited}");
        assert!(edited.contains("rev: bbbbbbb"), "{edited}");
        assert!(!edited.contains("aaaaaaa"), "{edited}");
    }

    #[test]
    fn the_edited_declaration_reads_back_as_the_new_one() {
        let new_spec = git_spec(Some("bbbbbbb"));
        let edited = edit_spec(SPEC, &new_spec).unwrap();
        let read = pin_from_yaml(&edited).unwrap().expect("a pin is declared");
        assert_eq!(read, new_spec);
    }

    #[test]
    fn a_block_in_the_middle_of_the_file_ends_where_the_next_key_starts() {
        let text = "name: shop\n\nthis:\n  source: git\n  url: https://github.com/this-rs/this\n  rev: aaaaaaa\n\ntargets: []\n";
        let edited = edit_spec(text, &git_spec(Some("bbbbbbb"))).unwrap();

        assert!(edited.contains("rev: bbbbbbb"), "{edited}");
        assert!(
            edited.contains("targets: []"),
            "the next key survives:\n{edited}"
        );
        assert!(!edited.contains("aaaaaaa"), "{edited}");
    }

    #[test]
    fn a_dependency_that_names_no_repository_is_refused() {
        let manifest = "[dependencies]\nthis = { package = \"this-rs\", path = \"../this\" }\n";
        let error = edit_manifest_rev(manifest, "bbbbbbb").unwrap_err();
        assert!(error.contains("does not name a repository"), "{error}");
    }

    #[test]
    fn a_file_without_a_declaration_gets_one_appended() {
        let edited = edit_spec(
            "name: shop\napi:\n  path: api\n",
            &git_spec(Some("bbbbbbb")),
        )
        .unwrap();
        let read = pin_from_yaml(&edited).unwrap().expect("a pin is declared");
        assert_eq!(read.source().unwrap().rev(), Some("bbbbbbb"));
    }

    #[test]
    fn a_lock_records_the_commit_it_replaces() {
        let source = Source::Git {
            url: "https://github.com/this-rs/this".into(),
            rev: Some("bbbbbbb".into()),
        };
        let lock = new_lock(
            &source,
            &["this-rs".to_string()],
            &["grpc".to_string()],
            Some("aaaaaaa"),
            None,
            NOW,
        )
        .unwrap();

        assert_eq!(lock.previous_rev.as_deref(), Some("aaaaaaa"));
        assert_eq!(lock.resolved_at, "2026-11-04T21:59:02Z");
        assert_eq!(lock.crates.len(), 1);
        assert_eq!(lock.crates[0].rev.as_deref(), Some("bbbbbbb"));
        assert_eq!(lock.features, vec!["grpc".to_string()]);
    }

    #[test]
    fn a_lock_cannot_be_written_without_a_commit() {
        let source = Source::Git {
            url: "https://github.com/this-rs/this".into(),
            rev: None,
        };
        assert!(new_lock(&source, &[], &[], None, None, NOW).is_err());
    }

    #[test]
    fn a_plan_moves_the_declaration_the_manifest_and_the_lock_together() {
        let spec = git_spec(Some("aaaaaaa"));
        let edits = plan(
            SPEC,
            MANIFEST,
            &spec,
            "bbbbbbb",
            Some("aaaaaaa"),
            &["this-rs".into()],
            NOW,
        )
        .unwrap();

        assert!(edits.specification.contains("rev: bbbbbbb"));
        assert!(edits.manifest.contains(r#"rev = "bbbbbbb""#));
        assert_eq!(edits.lock.previous_rev.as_deref(), Some("aaaaaaa"));
        assert_eq!(edits.lock.source.rev.as_deref(), Some("bbbbbbb"));
        assert_eq!(edits.lock.features, vec!["grpc".to_string()]);
    }

    #[test]
    fn a_local_path_pin_is_not_something_an_upgrade_can_move() {
        let spec = PinSpec {
            source: SourceFields {
                source: SourceKind::Path,
                path: Some("../this".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let error = plan(SPEC, MANIFEST, &spec, "bbbbbbb", None, &[], NOW).unwrap_err();
        assert!(error.contains("moves a git pin"), "{error}");
    }
}
