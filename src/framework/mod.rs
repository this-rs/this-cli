//! # The framework pin
//!
//! A this-rs project does not depend on "a version": it depends on a *contract*
//! that moves. Between two commits a field is born, a type dies, a lifecycle
//! decision reverses. So the interesting question is never "which version" but
//! "which commit, and what changed since".
//!
//! Two files answer it, and they are separate on purpose:
//!
//! | File | Says | Written by |
//! |---|---|---|
//! | `this.yaml` → [`pin::PinSpec`] | what the project *wants* | a human |
//! | `.this/framework.lock` → [`lock::Lock`] | what it *got* | the CLI |
//!
//! A channel (`stable`, `canary`, `next`) lives in the declaration; a commit
//! lives in the lock. Putting the channel in the lock would make it lie the
//! moment somebody resolved it elsewhere.
//!
//! ## One commit for the whole repository
//!
//! Cargo unifies a dependency by source: two commits of the same repository give
//! two builds of the same crate and types that do not match. [`lock::validate`]
//! therefore treats a mixed lock as an error, not a preference.
//!
//! ## What this module does *not* do
//!
//! No network, no git, no clock of its own: [`lock::validate`] takes the current
//! time as an argument, so every rule is testable to the millisecond.

pub mod digest;
pub mod lock;
pub mod manifest;
pub mod migration;
pub mod pin;
pub mod upgrade;

pub use lock::{Lock, this_dependency, validate};
pub use manifest::{dependency_from_manifest, manifest_drift};
pub use pin::{PinSpec, Source, normalized_features, short_rev};

use std::path::{Path, PathBuf};

/// Directory of the pin's evidence, at the root of a workspace.
pub const LOCK_DIR: &str = ".this";

/// File name of the resolution, inside [`LOCK_DIR`].
pub const LOCK_FILE: &str = "framework.lock";

/// Past this age a lock is reported as stale: the channel has likely moved.
pub const STALE_AFTER_DAYS: u64 = 30;

/// [`STALE_AFTER_DAYS`] in milliseconds.
pub const STALE_AFTER_MS: u64 = STALE_AFTER_DAYS * 86_400_000;

/// The crates.io version this CLI generates when a project is not pinned.
///
/// It is the *CLI's* knowledge, not the project's: bumped when the CLI ships
/// against a new published framework. Keeping it here rather than inside a
/// template is what stops the version from being one more hardcoded string.
pub const PUBLISHED_VERSION: &str = "0.0.9";

/// What a command line says about the framework.
#[derive(Clone, Copy, Debug, Default)]
pub struct PinRequest<'a> {
    /// A local checkout: development only.
    pub path: Option<&'a str>,
    /// A git repository.
    pub url: Option<&'a str>,
    /// The commit to pin, when the user names one.
    pub rev: Option<&'a str>,
    /// The channel to follow, when the user names one instead of a commit.
    pub channel: Option<&'a str>,
}

/// Turn a command-line request into a declaration.
///
/// One source, named once: asking for a path *and* a repository is a question
/// with two answers, so it is refused rather than silently prioritized.
pub fn pin_from_request(request: PinRequest<'_>, features: &[String]) -> Result<PinSpec, String> {
    if request.path.is_some() && request.url.is_some() {
        return Err(
            "`--this-path` and `--this-url` cannot both be given: the framework comes from one place"
                .to_string(),
        );
    }

    let mut spec = match (request.url, request.path) {
        (Some(url), _) => PinSpec {
            source: pin::SourceFields {
                source: pin::SourceKind::Git,
                url: Some(url.to_string()),
                rev: request.rev.map(str::to_string),
                ..Default::default()
            },
            channel: request.channel.map(pin::Channel::from_name).transpose()?,
            features: Vec::new(),
        },
        (None, Some(path)) => PinSpec {
            source: pin::SourceFields {
                source: pin::SourceKind::Path,
                path: Some(path.to_string()),
                ..Default::default()
            },
            channel: None,
            features: Vec::new(),
        },
        (None, None) => {
            if request.rev.is_some() {
                return Err("`--this-rev` only makes sense with `--this-url`".to_string());
            }
            if request.channel.is_some() {
                return Err("`--this-channel` only makes sense with `--this-url`".to_string());
            }
            PinSpec::crates_io(PUBLISHED_VERSION)
        }
    };

    spec.features = features.to_vec();
    Ok(spec)
}

/// Where the lock lives for a project rooted at `root`.
pub fn lock_path(root: &Path) -> PathBuf {
    root.join(LOCK_DIR).join(LOCK_FILE)
}

/// The crate manifest of this project, wherever it lives.
///
/// A standalone project keeps `Cargo.toml` at its root; a workspace keeps the
/// crate under `api.path` from `this.yaml` (`api` by default). Reading the root
/// unconditionally made every check on a workspace see an empty manifest — that
/// is, "nothing to report", which is the worst possible answer.
pub fn manifest_path(root: &Path) -> Option<PathBuf> {
    let direct = root.join("Cargo.toml");
    if direct.is_file() {
        return Some(direct);
    }

    let declared = std::fs::read_to_string(root.join("this.yaml"))
        .ok()
        .and_then(|text| serde_yaml::from_str::<serde_yaml::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("api")
                .and_then(|api| api.get("path"))
                .and_then(|path| path.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "api".to_string());

    let candidate = root.join(declared).join("Cargo.toml");
    candidate.is_file().then_some(candidate)
}

/// The `this` dependency line of a crate manifest, as written.
///
/// A project may be pinned without declaring anything (`--this-url` writes the
/// manifest directly). Reporting "not pinned" there would be a lie: the answer
/// lives in `Cargo.toml`, and saying so is cheaper than pretending.
pub fn manifest_dependency(manifest: &str) -> Option<String> {
    for line in manifest.lines() {
        let line = line.trim();
        // Deux noms : l'alias que le CLI génère (`this`) et le nom du crate,
        // que les projets existants utilisent (`this-rs`).
        let is_this = line.starts_with("this =")
            || line.starts_with("this=")
            || line.starts_with("this.workspace")
            || line.starts_with("this-rs =")
            || line.starts_with("this-rs.workspace");
        if is_this {
            return Some(line.to_string());
        }
    }
    None
}

/// Render the `this:` section of a `this.yaml` from a declaration.
///
/// Written by `init`, so a generated project declares its pin from birth. The
/// round trip is the contract: what this writes, [`read_spec`] must read back
/// as the same declaration — a test holds the two together.
pub fn spec_yaml(spec: &PinSpec) -> Result<String, String> {
    let body = serde_yaml::to_string(spec).map_err(|error| error.to_string())?;
    let mut out = String::from("this:\n");
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// Read the `this:` section of a `this.yaml`, without requiring the rest of the
/// workspace schema to be complete.
///
/// Extracted rather than added to [`crate::config::WorkspaceConfig`] on purpose:
/// a pin is a property of the project, not of the workspace's deployment
/// targets, and a partial `this.yaml` should still be able to declare one.
pub fn read_spec(root: &Path) -> Result<Option<PinSpec>, String> {
    let path = root.join("this.yaml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    pin_from_yaml(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The pure half of [`read_spec`]: what a `this.yaml` *text* declares.
pub fn pin_from_yaml(text: &str) -> Result<Option<PinSpec>, String> {
    let value: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    match value.get("this") {
        None | Some(serde_yaml::Value::Null) => Ok(None),
        Some(section) => serde_yaml::from_value(section.clone())
            .map(Some)
            .map_err(|e| format!("`this:` {e}")),
    }
}

/// Read the lock, if the project has one.
///
/// A missing file is `Ok(None)`, not an error: a project that names a published
/// version has nothing to resolve.
pub fn read_lock(root: &Path) -> Result<Option<Lock>, String> {
    let path = lock_path(root);
    match std::fs::read_to_string(&path) {
        Ok(text) => Lock::from_yaml(&text)
            .map(Some)
            .map_err(|error| format!("{}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Write the lock, creating `.this/` when needed.
///
/// Not called yet: the lock is written by `framework resolve` (see PIN-5). It
/// lives here so that the read and the write of the same file stay side by side.
#[allow(dead_code)]
pub fn write_lock(root: &Path, lock: &Lock) -> Result<(), String> {
    let path = lock_path(root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    let yaml = lock.to_yaml().map_err(|error| error.to_string())?;
    std::fs::write(&path, yaml).map_err(|error| format!("{}: {error}", path.display()))
}

/// Wall clock, epoch milliseconds. Kept out of [`validate`] so that rules stay
/// pure and a test can pin "now".
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Where the check runs. A local path pin is a working habit; in CI it is a
/// build that cannot happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    Local,
    Ci,
}

impl Run {
    /// CI if the environment says so, local otherwise.
    pub fn detect() -> Run {
        if std::env::var_os("CI").is_some() {
            Run::Ci
        } else {
            Run::Local
        }
    }
}

/// How bad a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// The project cannot be built as declared.
    Error,
    /// It builds, but something is worth knowing.
    Warn,
}

/// One finding about the pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: Level,
    /// Stable machine-readable code, so a CI can grep for it.
    pub code: &'static str,
    pub message: String,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            level: Level::Error,
            code,
            message: message.into(),
        }
    }

    pub fn warn(code: &'static str, message: impl Into<String>) -> Self {
        Diagnostic {
            level: Level::Warn,
            code,
            message: message.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.level == Level::Error
    }
}

/// How many findings block a build.
pub fn error_count(diagnostics: &[Diagnostic]) -> usize {
    diagnostics.iter().filter(|d| d.is_error()).count()
}

#[cfg(test)]
mod tests {
    use super::pin::Channel;
    use super::*;

    #[test]
    fn a_generated_project_without_a_path_gets_the_published_version() {
        let spec = pin_from_request(PinRequest::default(), &["grpc".to_string()]).unwrap();
        let line = this_dependency(&spec, None).unwrap();
        assert!(line.contains("version = \"0.0.9\""), "{line}");
        assert!(line.contains("\"grpc\""), "{line}");
    }

    #[test]
    fn a_generated_project_with_a_path_gets_a_path_dependency() {
        let spec = pin_from_request(
            PinRequest {
                path: Some("../this"),
                ..Default::default()
            },
            &[],
        )
        .unwrap();
        let line = this_dependency(&spec, None).unwrap();
        assert!(line.contains("path = \"../this\""), "{line}");
        assert!(!line.contains("version ="), "{line}");
        assert!(!line.contains("features ="), "no features, no key: {line}");
    }

    #[test]
    fn a_repository_and_a_commit_become_a_git_pin() {
        let spec = pin_from_request(
            PinRequest {
                url: Some("https://github.com/this-rs/this"),
                rev: Some("cceb3063e1"),
                channel: Some("stable"),
                ..Default::default()
            },
            &["grpc".to_string()],
        )
        .unwrap();
        let line = this_dependency(&spec, None).unwrap();
        assert!(
            line.contains("git = \"https://github.com/this-rs/this\""),
            "{line}"
        );
        assert!(line.contains("rev = \"cceb3063e1\""), "{line}");
    }

    #[test]
    fn a_path_and_a_repository_together_are_refused() {
        let error = pin_from_request(
            PinRequest {
                path: Some("../this"),
                url: Some("https://github.com/this-rs/this"),
                ..Default::default()
            },
            &[],
        )
        .unwrap_err();
        assert!(error.contains("one place"), "{error}");
    }

    #[test]
    fn a_commit_without_a_repository_is_refused() {
        let error = pin_from_request(
            PinRequest {
                rev: Some("cceb306"),
                ..Default::default()
            },
            &[],
        )
        .unwrap_err();
        assert!(error.contains("--this-url"), "{error}");
    }

    #[test]
    fn an_unknown_channel_is_refused_with_the_list() {
        let error = pin_from_request(
            PinRequest {
                url: Some("https://github.com/this-rs/this"),
                channel: Some("beta"),
                ..Default::default()
            },
            &[],
        )
        .unwrap_err();
        assert!(error.contains("stable, canary, next"), "{error}");
    }

    fn sample_lock() -> Lock {
        Lock {
            source: pin::SourceFields {
                source: pin::SourceKind::Git,
                url: Some("https://github.com/this-rs/this".into()),
                rev: Some("abc1234".into()),
                ..Default::default()
            },
            resolved_at: "2026-11-04T21:59:02Z".into(),
            channel: Some(pin::Channel::Stable),
            crates: vec![lock::LockedCrate::new("this-rs", Some("abc1234".into()))],
            features: vec!["grpc".into()],
            api_digest: None,
            migrations: vec!["0009-session-to-capabilities".into()],
            previous_rev: None,
        }
    }

    #[test]
    fn a_missing_lock_is_absence_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_lock(dir.path()).unwrap().is_none());
    }

    #[test]
    fn a_lock_survives_a_trip_through_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let lock = sample_lock();
        write_lock(dir.path(), &lock).unwrap();
        assert_eq!(read_lock(dir.path()).unwrap().unwrap(), lock);
        // Compare components, not a string: `display()` uses the separator of
        // the platform, so `".this/framework.lock"` is a Windows-only failure.
        let path = lock_path(dir.path());
        assert_eq!(path.file_name().unwrap(), LOCK_FILE);
        assert_eq!(path.parent().unwrap().file_name().unwrap(), LOCK_DIR);
    }

    #[test]
    fn an_unreadable_lock_is_an_error_not_a_silent_absence() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(LOCK_DIR)).unwrap();
        std::fs::write(lock_path(dir.path()), "source: [not, a, mapping").unwrap();
        assert!(read_lock(dir.path()).is_err());
    }

    #[test]
    fn a_spec_is_read_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let yaml = "name: shop\nthis:\n  source: crates-io\n  version: 0.0.9\n";
        std::fs::write(dir.path().join("this.yaml"), yaml).unwrap();
        let spec = read_spec(dir.path()).unwrap().expect("a pin is declared");
        assert_eq!(spec.source.version.as_deref(), Some("0.0.9"));
    }

    #[test]
    fn a_project_without_a_this_yaml_declares_no_pin() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_spec(dir.path()).unwrap().is_none());
    }

    #[test]
    fn a_broken_this_yaml_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("this.yaml"), "this: [not, a, mapping").unwrap();
        assert!(read_spec(dir.path()).is_err());
    }

    #[test]
    fn a_workspace_crate_is_found_under_the_path_its_this_yaml_declares() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("serveur")).unwrap();
        std::fs::write(
            dir.path().join("this.yaml"),
            "name: shop\napi:\n  path: serveur\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("serveur/Cargo.toml"), "[dependencies]\n").unwrap();

        let found = manifest_path(dir.path()).expect("the crate manifest is found");
        assert_eq!(found, dir.path().join("serveur/Cargo.toml"));
    }

    #[test]
    fn a_standalone_crate_is_found_at_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[dependencies]\n").unwrap();
        assert_eq!(
            manifest_path(dir.path()),
            Some(dir.path().join("Cargo.toml"))
        );
    }

    #[test]
    fn a_project_without_a_manifest_reports_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("this.yaml"),
            "name: shop\napi:\n  path: api\n",
        )
        .unwrap();
        assert!(manifest_path(dir.path()).is_none());
    }

    #[test]
    fn the_manifest_dependency_is_found_wherever_it_sits() {
        let manifest = "[package]\nname = \"shop\"\n\n[dependencies]\nanyhow = \"1\"\nthis = { package = \"this-rs\", git = \"https://github.com/this-rs/this\", rev = \"cceb306\" }\n";
        let line = manifest_dependency(manifest).expect("the dependency is there");
        assert!(line.starts_with("this ="), "{line}");
        assert!(line.contains("rev = \"cceb306\""), "{line}");
    }

    #[test]
    fn a_manifest_without_the_dependency_reports_nothing() {
        assert!(manifest_dependency("[dependencies]\nanyhow = \"1\"\n").is_none());
    }

    #[test]
    fn what_a_declaration_writes_is_what_it_reads_back() {
        let cases = [
            PinSpec::crates_io("0.0.9"),
            PinSpec {
                source: pin::SourceFields {
                    source: pin::SourceKind::Git,
                    url: Some("https://github.com/this-rs/this".into()),
                    rev: Some("cceb306".into()),
                    ..Default::default()
                },
                channel: Some(pin::Channel::Stable),
                features: vec!["grpc".into(), "websocket".into()],
            },
            PinSpec {
                source: pin::SourceFields {
                    source: pin::SourceKind::Git,
                    url: Some("https://github.com/this-rs/this".into()),
                    ..Default::default()
                },
                channel: Some(pin::Channel::Canary),
                features: vec![],
            },
            PinSpec {
                source: pin::SourceFields {
                    source: pin::SourceKind::Path,
                    path: Some("../this".into()),
                    ..Default::default()
                },
                channel: None,
                features: vec![],
            },
        ];

        for spec in cases {
            let yaml = format!(
                "name: shop\napi:\n  path: api\n{}",
                spec_yaml(&spec).unwrap()
            );
            let read = pin_from_yaml(&yaml).unwrap().expect("a pin is declared");
            assert_eq!(read, spec, "round trip failed for:\n{yaml}");
        }
    }

    #[test]
    fn a_pin_is_read_from_this_yaml_and_the_rest_is_ignored() {
        let yaml = "name: shop\napi:\n  path: api\nthis:\n  source: git\n  url: https://github.com/this-rs/this\n  channel: stable\n";
        let spec = pin_from_yaml(yaml).unwrap().expect("a pin is declared");
        assert_eq!(spec.channel, Some(Channel::Stable));
        assert_eq!(
            spec.source.url.as_deref(),
            Some("https://github.com/this-rs/this")
        );
    }

    #[test]
    fn a_this_yaml_without_the_section_declares_no_pin() {
        assert!(
            pin_from_yaml("name: shop\napi:\n  path: api\n")
                .unwrap()
                .is_none()
        );
        assert!(pin_from_yaml("name: shop\nthis:\n").unwrap().is_none());
    }
}
