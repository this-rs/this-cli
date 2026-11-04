//! The vocabulary of the framework at the commit a project pins.
//!
//! The point is to stop answering "which crate, which feature?" from memory.
//! A feature that does not exist at the pinned commit fails much later, in
//! `cargo build`, with a message about an unknown feature — after the model, or
//! the user, has already written the manifest.
//!
//! What is read: the manifests. `[features]` per crate is exactly what Cargo
//! will accept as a feature, so it is exactly what the answer must be. No
//! guessing, and no parsing of Rust source: a *declared* feature list is
//! authoritative, a scanned symbol is not.
//!
//! Where it is read from: a checkout on this machine. A `path` pin already
//! names one. For a git pin there is none until Cargo has fetched that commit —
//! and this module says so instead of inventing a vocabulary.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::framework::pin::Source;

/// One crate of the framework, as its manifest declares it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateDigest {
    pub name: String,
    pub version: String,
    /// Sorted, as Cargo accepts them.
    pub features: Vec<String>,
}

/// The framework's vocabulary at one commit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Digest {
    /// The commit it was read at, when the pin names one.
    pub rev: Option<String>,
    /// Where it was read, for a human to check.
    pub checkout: String,
    /// Sorted by name.
    pub crates: Vec<CrateDigest>,
}

impl Digest {
    /// The features of one crate, if the framework has such a crate.
    pub fn features_of(&self, crate_name: &str) -> Option<&[String]> {
        self.crates
            .iter()
            .find(|krate| krate.name == crate_name)
            .map(|krate| krate.features.as_slice())
    }

    /// Whether `crate_name` can be built with exactly these features.
    pub fn accepts(&self, crate_name: &str, features: &[String]) -> Result<(), Vec<String>> {
        let Some(known) = self.features_of(crate_name) else {
            return Err(vec![format!(
                "crate `{crate_name}` is not part of the framework at this commit"
            )]);
        };
        let unknown: Vec<String> = features
            .iter()
            .filter(|feature| !known.contains(feature))
            .cloned()
            .collect();
        if unknown.is_empty() {
            Ok(())
        } else {
            Err(unknown)
        }
    }
}

/// A crate, read from its own manifest text.
pub fn crate_from_manifest(text: &str) -> Result<CrateDigest, String> {
    let document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| error.to_string())?;

    let package = document
        .get("package")
        .ok_or_else(|| "no [package]: this is not a crate".to_string())?;

    let name = package
        .get("name")
        .and_then(|name| name.as_str())
        .ok_or_else(|| "no `package.name`".to_string())?
        .to_string();

    let version = package
        .get("version")
        .and_then(|version| version.as_str())
        .unwrap_or("0.0.0")
        .to_string();

    let mut features: Vec<String> = document
        .get("features")
        .and_then(|features| features.as_table_like())
        .map(|table| table.iter().map(|(key, _)| key.to_string()).collect())
        .unwrap_or_default();
    features.sort();

    Ok(CrateDigest {
        name,
        version,
        features,
    })
}

/// The members a workspace manifest declares, or `["."]` for a lone crate.
pub fn members_from_manifest(text: &str) -> Result<Vec<String>, String> {
    let document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| error.to_string())?;

    let members = document
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(|members| members.as_array())
        .map(|members| {
            members
                .iter()
                .filter_map(|member| member.as_str().map(str::to_string))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    if members.is_empty() {
        Ok(vec![".".to_string()])
    } else {
        Ok(members)
    }
}

/// Read the framework's vocabulary from a checkout on this machine.
pub fn digest_from_checkout(checkout: &Path, rev: Option<&str>) -> Result<Digest, String> {
    let root_manifest_path = checkout.join("Cargo.toml");
    let root_manifest = std::fs::read_to_string(&root_manifest_path)
        .map_err(|error| format!("{}: {error}", root_manifest_path.display()))?;

    let mut crates = Vec::new();
    for member in members_from_manifest(&root_manifest)? {
        let manifest = if member == "." {
            root_manifest_path.clone()
        } else {
            checkout.join(&member).join("Cargo.toml")
        };
        // A declared member may not exist in an older checkout: skip, do not fail.
        if let Ok(text) = std::fs::read_to_string(&manifest)
            && let Ok(krate) = crate_from_manifest(&text)
        {
            crates.push(krate);
        }
    }

    crates.sort_by(|a, b| a.name.cmp(&b.name));
    crates.dedup_by(|a, b| a.name == b.name);

    if crates.is_empty() {
        return Err(format!(
            "{}: no crate could be read there",
            checkout.display()
        ));
    }

    // Canonical when possible: `../framework` is hard to check by eye, and the
    // point of this field is that a human can verify where the answer came from.
    let shown = checkout
        .canonicalize()
        .unwrap_or_else(|_| checkout.to_path_buf());

    Ok(Digest {
        rev: rev.map(str::to_string),
        checkout: shown.display().to_string(),
        crates,
    })
}

/// Where the pinned commit can be read, if it is on this machine.
///
/// A `path` pin names its checkout, so it is read directly. Anything else is a
/// repository: reading it would mean fetching it, which this command does not
/// do — it answers, or it says it cannot answer.
pub fn checkout_for(project_root: &Path, source: &Source) -> Option<PathBuf> {
    match source {
        Source::Path { path } => {
            let candidate = project_root.join(path);
            candidate.is_dir().then_some(candidate)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"[package]
name = "this-rs"
version = "0.0.10"

[features]
default = ["in-memory"]
in-memory = []
grpc = ["tonic"]
websocket = []
"#;

    #[test]
    fn a_crate_is_read_with_its_features_in_order() {
        let krate = crate_from_manifest(MANIFEST).unwrap();
        assert_eq!(krate.name, "this-rs");
        assert_eq!(krate.version, "0.0.10");
        assert_eq!(
            krate.features,
            vec!["default", "grpc", "in-memory", "websocket"]
        );
    }

    #[test]
    fn a_crate_without_features_has_an_empty_list() {
        let krate = crate_from_manifest("[package]\nname = \"x\"\nversion = \"1.0.0\"\n").unwrap();
        assert!(krate.features.is_empty());
        assert_eq!(krate.version, "1.0.0");
    }

    #[test]
    fn a_manifest_that_is_not_a_crate_is_refused() {
        assert!(crate_from_manifest("[dependencies]\nanyhow = \"1\"\n").is_err());
    }

    #[test]
    fn a_workspace_lists_its_members() {
        let text = "[workspace]\nmembers = [\".\", \"crates/this-descriptor\"]\n";
        assert_eq!(
            members_from_manifest(text).unwrap(),
            vec![".".to_string(), "crates/this-descriptor".to_string()]
        );
    }

    #[test]
    fn a_lone_crate_is_its_own_member() {
        assert_eq!(
            members_from_manifest(MANIFEST).unwrap(),
            vec![".".to_string()]
        );
    }

    #[test]
    fn a_checkout_is_read_across_its_members() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\".\", \"crates/helper\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("crates/helper")).unwrap();
        std::fs::write(
            dir.path().join("crates/helper/Cargo.toml"),
            "[package]\nname = \"helper\"\nversion = \"0.0.1\"\n\n[features]\nx = []\n",
        )
        .unwrap();
        // The root is a workspace only: no [package], so it contributes nothing.
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\".\", \"crates/helper\"]\n[package]\nname = \"root\"\nversion = \"0.1.0\"\n\n[features]\ndefault = []\n",
        )
        .unwrap();

        let digest = digest_from_checkout(dir.path(), Some("cceb306")).unwrap();

        assert_eq!(digest.rev.as_deref(), Some("cceb306"));
        assert_eq!(
            digest
                .crates
                .iter()
                .map(|k| k.name.as_str())
                .collect::<Vec<_>>(),
            vec!["helper", "root"]
        );
        assert_eq!(digest.features_of("helper"), Some(&["x".to_string()][..]));
    }

    #[test]
    fn a_checkout_that_declares_nothing_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"gone\"]\n",
        )
        .unwrap();
        assert!(digest_from_checkout(dir.path(), None).is_err());
    }

    #[test]
    fn a_feature_the_commit_does_not_have_is_named() {
        let digest = Digest {
            rev: None,
            checkout: ".".into(),
            crates: vec![crate_from_manifest(MANIFEST).unwrap()],
        };

        assert!(digest.accepts("this-rs", &["grpc".to_string()]).is_ok());

        let unknown = digest
            .accepts("this-rs", &["grpc".to_string(), "frontend".to_string()])
            .unwrap_err();
        assert_eq!(unknown, vec!["frontend".to_string()]);
    }

    #[test]
    fn a_crate_outside_the_framework_is_named_as_absent() {
        let digest = Digest {
            rev: None,
            checkout: ".".into(),
            crates: vec![],
        };
        let error = digest.accepts("this-rs", &[]).unwrap_err();
        assert!(error[0].contains("not part of the framework"));
    }

    #[test]
    fn only_a_local_checkout_can_be_read() {
        let dir = tempfile::tempdir().unwrap();
        let git = Source::Git {
            url: "https://github.com/this-rs/this".into(),
            rev: Some("cceb306".into()),
        };
        assert!(checkout_for(dir.path(), &git).is_none());

        std::fs::create_dir_all(dir.path().join("vendor/this")).unwrap();
        let path = Source::Path {
            path: "vendor/this".into(),
        };
        assert_eq!(
            checkout_for(dir.path(), &path),
            Some(dir.path().join("vendor/this"))
        );
    }
}
