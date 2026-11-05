//! What the crate manifest actually builds.
//!
//! The declaration says what a project wants, the lock says what it resolved —
//! but neither is what Cargo compiles. That is `Cargo.toml`, and until this
//! module existed nothing compared it to the other two: a project could declare
//! one commit, record that same commit in its lock, and build a third.
//!
//! So the manifest is read back into the **same vocabulary** as a pin
//! ([`Source`]), which is what makes the comparison a comparison of equals
//! rather than a string hunt.

use crate::framework::Diagnostic;
use crate::framework::pin::Source;

/// The names the framework can be depended on under.
///
/// The CLI generates `this = { package = "this-rs", ... }` — an alias. Existing
/// projects (this-examples' own examples) depend on `this-rs` directly. Reading
/// only the alias made `check` see no dependency at all, and answer "consistent"
/// on a project it had not read: measured on `examples/websocket`.
const NAMES: [&str; 2] = ["this", "this-rs"];

/// The `this` dependency item, whatever it is called.
fn this_dependency(document: &toml_edit::DocumentMut) -> Option<&toml_edit::Item> {
    let dependencies = document.get("dependencies")?.as_table_like()?;
    NAMES.iter().find_map(|name| dependencies.get(name))
}

/// The `this` dependency of a crate manifest, parsed like a pin.
///
/// `Ok(None)` means "nothing to read here": no dependency, or one inherited
/// from a workpace (`this.workspace = true`), whose value lives elsewhere.
pub fn dependency_from_manifest(manifest: &str) -> Result<Option<Source>, String> {
    let document = manifest
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| error.to_string())?;

    let Some(dependency) = this_dependency(&document) else {
        return Ok(None);
    };

    dependency_as_source(dependency)
}

/// The crate the `this` dependency names, when it names one.
///
/// `this = { package = "this-rs", ... }` is how a project depends on the
/// framework under another name; without it the crate is called `this`.
pub fn dependency_package(manifest: &str) -> Option<String> {
    let document = manifest.parse::<toml_edit::DocumentMut>().ok()?;
    let dependency = this_dependency(&document)?;
    dependency
        .as_table_like()?
        .get("package")
        .and_then(|package| package.as_str())
        .map(str::to_string)
}

/// Une source dont le chemin est résolu, pour comparer deux façons d'écrire le
/// même endroit.
///
/// Mesuré sur `examples/websocket` : le manifeste racine du workspace écrit
/// `path = "../this"`, la déclaration du projet écrit `path = "../../../this"`
/// — le même dossier, deux bases. Comparées comme des chaînes, elles
/// produisaient un `manifest-spec-drift` qui n'existait pas.
pub fn resolve_paths(source: Source, base: &std::path::Path) -> Source {
    match source {
        Source::Path { path } => {
            let joined = base.join(&path);
            let resolved = joined.canonicalize().unwrap_or(joined);
            Source::Path {
                path: resolved.display().to_string(),
            }
        }
        other => other,
    }
}

/// Whether the `this` dependency is inherited from a workspace.
///
/// `this = { workspace = true }` names no source: the value lives in the root
/// manifest's `[workspace.dependencies]`. Returning `Ok(None)` for it, as the
/// reader used to, made `check` conclude "consistent" without having read
/// anything — measured on a real generated example.
pub fn inherits_this(manifest: &str) -> bool {
    let Ok(document) = manifest.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    this_dependency(&document)
        .and_then(|dependency| dependency.as_table_like())
        .and_then(|table| table.get("workspace"))
        .and_then(|workspace| workspace.as_bool())
        == Some(true)
}

/// The `this` dependency of the nearest enclosing workspace manifest.
///
/// Walks up from the crate: a workspace root is a manifest with a `[workspace]`
/// table. The first one found decides — if it declares no `this` under
/// `[workspace.dependencies]`, the answer is `Ok(None)` rather than a guess from
/// further up.
pub fn workspace_dependency(start: &std::path::Path) -> Result<Option<Source>, String> {
    let mut current = start;
    loop {
        let candidate = current.join("Cargo.toml");
        if let Ok(text) = std::fs::read_to_string(&candidate)
            && let Ok(document) = text.parse::<toml_edit::DocumentMut>()
            && let Some(workspace) = document.get("workspace")
        {
            let dependency = workspace
                .get("dependencies")
                .and_then(|dependencies| dependencies.as_table_like())
                .and_then(|dependencies| NAMES.iter().find_map(|name| dependencies.get(name)));
            let Some(dependency) = dependency else {
                return Ok(None);
            };
            let source = dependency_as_source(dependency)?;
            return Ok(source.map(|source| resolve_paths(source, current)));
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => return Ok(None),
        }
    }
}

/// One `[dependencies]` entry, read as a source.
fn dependency_as_source(dependency: &toml_edit::Item) -> Result<Option<Source>, String> {
    // `this = "0.0.9"`
    if let Some(version) = dependency.as_str() {
        return Ok(Some(Source::CratesIo {
            version: version.to_string(),
        }));
    }

    let Some(table) = dependency.as_table_like() else {
        return Err("the `this` dependency is neither a version string nor a table".to_string());
    };

    if table.get("workspace").and_then(|value| value.as_bool()) == Some(true) {
        return Ok(None);
    }

    let field = |name: &str| {
        table
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::to_string)
    };

    if let Some(path) = field("path") {
        return Ok(Some(Source::Path { path }));
    }
    if let Some(url) = field("git") {
        return Ok(Some(Source::Git {
            url,
            rev: field("rev"),
        }));
    }
    if let Some(version) = field("version") {
        return Ok(Some(Source::CratesIo { version }));
    }

    Err("the `this` dependency names neither a version, a path nor a repository".to_string())
}

/// How the manifest (what Cargo builds) disagrees with the pin.
///
/// `spec` is the declaration, `lock` the recorded resolution; both are optional
/// because a project may have neither.
pub fn manifest_drift(
    manifest: &Source,
    spec: Option<&Source>,
    lock: Option<&Source>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    if let Source::Git { url, rev: None } = manifest {
        out.push(Diagnostic::warn(
            "manifest-branch-pin",
            format!(
                "`this` comes from {url} without a commit: Cargo follows a branch, so the build moves under the project"
            ),
        ));
    }

    if let Some(lock) = lock
        && !names_the_same(manifest, lock)
    {
        out.push(Diagnostic::error(
            "manifest-lock-drift",
            format!(
                "the manifest builds {} but the lock records {}: the recorded commit is not the one compiled",
                manifest.describe(),
                lock.describe()
            ),
        ));
    }

    if let Some(spec) = spec
        && SourceMismatch::between(manifest, spec).is_some()
    {
        out.push(Diagnostic::warn(
            "manifest-spec-drift",
            format!(
                "the manifest builds {} but this.yaml declares {}",
                manifest.describe(),
                spec.describe()
            ),
        ));
    }

    out
}

/// Two sources name the same artifact.
///
/// A git source without a commit matches nothing: an unresolved url could be
/// any commit, so pretending it equals a resolved one would hide exactly the
/// defect this module is for.
pub fn names_the_same(left: &Source, right: &Source) -> bool {
    match (left, right) {
        (Source::CratesIo { version: a }, Source::CratesIo { version: b }) => a == b,
        (Source::Path { path: a }, Source::Path { path: b }) => a == b,
        (
            Source::Git {
                url: a,
                rev: Some(a_rev),
            },
            Source::Git {
                url: b,
                rev: Some(b_rev),
            },
        ) => a == b && a_rev == b_rev,
        _ => false,
    }
}

/// What differs between two sources, when the difference is worth naming.
enum SourceMismatch {
    /// Not the same repository, path or version.
    Family,
    /// The same repository at another commit.
    Commit,
}

impl SourceMismatch {
    fn between(manifest: &Source, spec: &Source) -> Option<SourceMismatch> {
        match (manifest, spec) {
            (Source::CratesIo { version: a }, Source::CratesIo { version: b }) => {
                (a != b).then_some(SourceMismatch::Family)
            }
            (Source::Path { path: a }, Source::Path { path: b }) => {
                (a != b).then_some(SourceMismatch::Family)
            }
            (
                Source::Git {
                    url: a,
                    rev: manifest_rev,
                },
                Source::Git {
                    url: b,
                    rev: spec_rev,
                },
            ) => {
                if a != b {
                    return Some(SourceMismatch::Family);
                }
                // A declaration without a commit follows a channel: whatever the
                // manifest pinned is a valid resolution of it.
                match (manifest_rev, spec_rev) {
                    (Some(manifest_rev), Some(spec_rev)) if manifest_rev != spec_rev => {
                        Some(SourceMismatch::Commit)
                    }
                    _ => None,
                }
            }
            _ => Some(SourceMismatch::Family),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_package_the_dependency_names_is_read() {
        let manifest = "[dependencies]\nthis = { package = \"this-rs\", version = \"0.0.9\" }\n";
        assert_eq!(dependency_package(manifest).as_deref(), Some("this-rs"));

        let renamed = "[dependencies]\nthis = \"0.0.9\"\n";
        assert_eq!(dependency_package(renamed), None);

        assert_eq!(dependency_package("[dependencies]\nanyhow = \"1\"\n"), None);
    }

    #[test]
    fn an_inherited_dependency_is_recognised() {
        let inherited = "[dependencies]\nthis = { workspace = true }\n";
        assert!(inherits_this(inherited));

        let direct = "[dependencies]\nthis = { package = \"this-rs\", version = \"0.0.9\" }\n";
        assert!(!inherits_this(direct));
        assert!(!inherits_this("[dependencies]\nthis = \"0.0.9\"\n"));
    }

    #[test]
    fn a_manifest_that_is_not_toml_inherits_nothing() {
        assert!(!inherits_this("this = { workspace = true "));
    }

    #[test]
    fn the_workspace_root_is_walked_up_to() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"api\"]\n\n[workspace.dependencies]\nthis = { package = \"this-rs\", path = \"../this\", features = [\"grpc\"] }\n",
        )
        .unwrap();
        let api = dir.path().join("api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(
            api.join("Cargo.toml"),
            "[dependencies]\nthis = { workspace = true }\n",
        )
        .unwrap();

        // Le chemin est résolu : `../this` écrit depuis la racine du workspace
        // devient un chemin absolu, pour être comparable à celui du projet.
        let raw = dir.path().join("../this");
        let expected = raw.canonicalize().unwrap_or(raw);
        assert_eq!(
            workspace_dependency(&api).unwrap(),
            Some(Source::Path {
                path: expected.display().to_string()
            })
        );
    }

    #[test]
    fn a_workspace_that_declares_nothing_answers_none_rather_than_guessing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        let api = dir.path().join("api");
        std::fs::create_dir_all(&api).unwrap();

        assert_eq!(workspace_dependency(&api).unwrap(), None);
    }

    #[test]
    fn no_workspace_above_answers_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(workspace_dependency(dir.path()).unwrap(), None);
    }

    #[test]
    fn a_plain_version_string_is_read() {
        let manifest = "[package]\nname = \"api\"\n\n[dependencies]\nthis = \"0.0.9\"\n";
        assert_eq!(
            dependency_from_manifest(manifest).unwrap(),
            Some(Source::CratesIo {
                version: "0.0.9".into()
            })
        );
    }

    #[test]
    fn a_version_inside_a_table_is_read() {
        let manifest = "[dependencies]\nthis = { package = \"this-rs\", version = \"0.0.9\", features = [\"grpc\"] }\n";
        assert_eq!(
            dependency_from_manifest(manifest).unwrap(),
            Some(Source::CratesIo {
                version: "0.0.9".into()
            })
        );
    }

    #[test]
    fn a_repository_and_its_commit_are_read() {
        let manifest = "[dependencies]\nthis = { git = \"https://github.com/this-rs/this\", rev = \"cceb306\" }\n";
        assert_eq!(
            dependency_from_manifest(manifest).unwrap(),
            Some(Source::Git {
                url: "https://github.com/this-rs/this".into(),
                rev: Some("cceb306".into()),
            })
        );
    }

    #[test]
    fn a_branch_is_read_as_no_commit_at_all() {
        let manifest = "[dependencies]\nthis = { git = \"https://github.com/this-rs/this\", branch = \"main\" }\n";
        assert_eq!(
            dependency_from_manifest(manifest).unwrap(),
            Some(Source::Git {
                url: "https://github.com/this-rs/this".into(),
                rev: None,
            })
        );
    }

    #[test]
    fn a_local_path_is_read() {
        let manifest = "[dependencies]\nthis = { package = \"this-rs\", path = \"../this\" }\n";
        assert_eq!(
            dependency_from_manifest(manifest).unwrap(),
            Some(Source::Path {
                path: "../this".into()
            })
        );
    }

    #[test]
    fn an_inherited_dependency_has_nothing_to_read_here() {
        let manifest = "[dependencies]\nthis = { workspace = true }\n";
        assert!(dependency_from_manifest(manifest).unwrap().is_none());
    }

    #[test]
    fn a_manifest_without_the_dependency_has_nothing_to_read() {
        assert!(
            dependency_from_manifest("[dependencies]\nanyhow = \"1\"\n")
                .unwrap()
                .is_none()
        );
        assert!(
            dependency_from_manifest("[package]\nname = \"x\"\n")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_dependency_that_names_no_source_is_refused() {
        let manifest = "[dependencies]\nthis = { package = \"this-rs\" }\n";
        assert!(dependency_from_manifest(manifest).is_err());
    }

    #[test]
    fn a_manifest_that_is_not_toml_is_an_error() {
        assert!(dependency_from_manifest("this = ").is_err());
    }

    // ---- manifest_drift -------------------------------------------------

    fn git(rev: Option<&str>) -> Source {
        Source::Git {
            url: "https://github.com/this-rs/this".into(),
            rev: rev.map(str::to_string),
        }
    }

    fn codes(found: &[Diagnostic]) -> Vec<&'static str> {
        found.iter().map(|d| d.code).collect()
    }

    #[test]
    fn a_manifest_that_agrees_with_the_lock_has_nothing_to_say() {
        assert!(manifest_drift(&git(Some("abc")), None, Some(&git(Some("abc")))).is_empty());
    }

    #[test]
    fn a_manifest_building_another_commit_than_the_lock_is_an_error() {
        let found = manifest_drift(&git(Some("abc")), None, Some(&git(Some("def"))));
        assert_eq!(codes(&found), vec!["manifest-lock-drift"]);
        assert!(found[0].is_error());
    }

    #[test]
    fn a_branch_in_the_manifest_is_named_as_such() {
        let found = manifest_drift(&git(None), None, None);
        assert_eq!(codes(&found), vec!["manifest-branch-pin"]);
        assert!(!found[0].is_error());
    }

    #[test]
    fn a_branch_never_counts_as_matching_a_recorded_commit() {
        let found = manifest_drift(&git(None), None, Some(&git(Some("abc"))));
        assert!(codes(&found).contains(&"manifest-lock-drift"));
    }

    #[test]
    fn a_declaration_without_a_commit_accepts_whatever_the_manifest_pinned() {
        assert!(manifest_drift(&git(Some("abc")), Some(&git(None)), None).is_empty());
    }

    #[test]
    fn a_manifest_commit_that_contradicts_the_declaration_is_warned_about() {
        let found = manifest_drift(&git(Some("abc")), Some(&git(Some("def"))), None);
        assert_eq!(codes(&found), vec!["manifest-spec-drift"]);
    }

    #[test]
    fn another_family_than_the_declaration_is_a_drift() {
        let manifest = Source::CratesIo {
            version: "0.0.9".into(),
        };
        let found = manifest_drift(&manifest, Some(&git(Some("abc"))), None);
        assert_eq!(codes(&found), vec!["manifest-spec-drift"]);
    }

    #[test]
    fn two_versions_that_differ_are_a_drift_and_two_that_match_are_not() {
        let manifest = Source::CratesIo {
            version: "0.0.10".into(),
        };
        let declared = Source::CratesIo {
            version: "0.0.9".into(),
        };
        assert_eq!(
            codes(&manifest_drift(&manifest, Some(&declared), None)),
            vec!["manifest-spec-drift"]
        );
        let same = Source::CratesIo {
            version: "0.0.10".into(),
        };
        assert!(manifest_drift(&manifest, Some(&same), None).is_empty());
    }
}
