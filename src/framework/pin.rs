//! What a project *declares* it targets.
//!
//! The declaration lives in `this.yaml`, under `this:`. It is deliberately flat
//! so it reads well in a file a human edits:
//!
//! ```yaml
//! this:
//!   source: git
//!   url: https://github.com/this-rs/this
//!   channel: stable
//!   features: [grpc, graphql]
//! ```
//!
//! The section is called `this`, never `framework`: `framework:` already means
//! the *frontend* framework (`react`) in [`crate::config::TargetConfig`].
//!
//! A declaration is untrusted: fields may be missing, or disagree with the
//! source kind. [`SourceFields::to_source`] turns it into a [`Source`] that
//! cannot be incomplete, so everything downstream (lock writing, Cargo line
//! rendering) is infallible.

use serde::{Deserialize, Serialize};

/// Which kind of thing the framework comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    /// A published version on crates.io. The only mode that names no commit.
    /// Also the default, because a project with no `this:` section is exactly
    /// this case.
    #[default]
    CratesIo,
    /// A local checkout. Development only: nothing reproduces another machine.
    Path,
    /// A git repository, resolved to a commit.
    Git,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::CratesIo => "crates-io",
            SourceKind::Path => "path",
            SourceKind::Git => "git",
        }
    }
}

/// The channel a project follows. A channel resolves to a commit *on the
/// developer's machine*; it is never stored as if it were the answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Channel {
    /// Tagged releases.
    #[default]
    Stable,
    /// A release candidate.
    Canary,
    /// In development.
    Next,
}

impl Channel {
    /// Parse the name a user typed. Refused with the list of what exists,
    /// rather than silently falling back to `stable`.
    pub fn from_name(name: &str) -> Result<Channel, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "stable" => Ok(Channel::Stable),
            "canary" => Ok(Channel::Canary),
            "next" => Ok(Channel::Next),
            other => Err(format!(
                "unknown channel `{other}`: expected one of stable, canary, next"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Canary => "canary",
            Channel::Next => "next",
        }
    }
}

/// The raw, possibly incomplete fields read from a file.
///
/// Shared by the declaration and the lock: both are edited by hand often enough
/// that they should read the same.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFields {
    pub source: SourceKind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
}

/// A declaration that checks out: every field the kind needs is present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    CratesIo { version: String },
    Path { path: String },
    Git { url: String, rev: Option<String> },
}

impl SourceFields {
    /// A one-line description that tolerates missing fields: used where the
    /// point is to show what a file says, not to refuse it.
    pub fn describe_hint(&self) -> String {
        match self.source {
            SourceKind::CratesIo => format!(
                "crates.io {}",
                self.version.as_deref().unwrap_or("(version missing)")
            ),
            SourceKind::Path => {
                format!("path {}", self.path.as_deref().unwrap_or("(path missing)"))
            }
            SourceKind::Git => match self.url.as_deref() {
                Some(url) => match self.rev.as_deref() {
                    Some(rev) => format!("{url} @ {}", short_rev(rev)),
                    None => format!("{url} (unresolved)"),
                },
                None => "git (url missing)".to_string(),
            },
        }
    }

    /// Validate the fields and collapse them into a [`Source`].
    pub fn to_source(&self) -> Result<Source, String> {
        match self.source {
            SourceKind::CratesIo => {
                let version = self
                    .version
                    .clone()
                    .ok_or_else(|| "source `crates-io` needs a `version`".to_string())?;
                Ok(Source::CratesIo { version })
            }
            SourceKind::Path => {
                let path = self
                    .path
                    .clone()
                    .ok_or_else(|| "source `path` needs a `path`".to_string())?;
                Ok(Source::Path { path })
            }
            SourceKind::Git => {
                let url = self
                    .url
                    .clone()
                    .ok_or_else(|| "source `git` needs a `url`".to_string())?;
                Ok(Source::Git {
                    url,
                    rev: self.rev.clone(),
                })
            }
        }
    }
}

impl Source {
    pub fn kind(&self) -> SourceKind {
        match self {
            Source::CratesIo { .. } => SourceKind::CratesIo,
            Source::Path { .. } => SourceKind::Path,
            Source::Git { .. } => SourceKind::Git,
        }
    }

    pub fn rev(&self) -> Option<&str> {
        match self {
            Source::Git { rev, .. } => rev.as_deref(),
            _ => None,
        }
    }

    /// Whether another machine can rebuild the exact same code from this.
    ///
    /// A version qualifies (crates.io is immutable), a commit qualifies, a
    /// local path never does.
    pub fn is_reproducible(&self) -> bool {
        match self {
            Source::CratesIo { .. } => true,
            Source::Git { rev, .. } => rev.is_some(),
            Source::Path { .. } => false,
        }
    }

    /// One line for a human: what this source is, in as few words as possible.
    pub fn describe(&self) -> String {
        match self {
            Source::CratesIo { version } => format!("crates.io {version}"),
            Source::Path { path } => format!("path {path}"),
            Source::Git { url, rev } => match rev {
                Some(rev) => format!("{url} @ {}", short_rev(rev)),
                None => format!("{url} (unresolved)"),
            },
        }
    }
}

/// The `this:` section of `this.yaml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinSpec {
    #[serde(flatten)]
    pub source: SourceFields,

    /// Which line this project follows. Only meaningful for `git`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,

    /// Cargo features requested from the framework. Stored as declared, rendered
    /// normalized so the output does not depend on the order they were typed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

impl PinSpec {
    /// A declaration with no `this:` section at all: the historical behaviour,
    /// a published version.
    pub fn crates_io(version: impl Into<String>) -> Self {
        PinSpec {
            source: SourceFields {
                source: SourceKind::CratesIo,
                version: Some(version.into()),
                ..Default::default()
            },
            channel: None,
            features: Vec::new(),
        }
    }

    pub fn source(&self) -> Result<Source, String> {
        self.source.to_source()
    }
}

/// Sort and dedupe, so two projects that ask for the same features in a
/// different order render byte-identical manifests.
pub fn normalized_features(features: &[String]) -> Vec<String> {
    let mut out: Vec<String> = features
        .iter()
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The first feature declared twice, if any. Reported rather than silently
/// merged: asking twice is a typo, not an intent.
pub fn first_duplicate(features: &[String]) -> Option<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut duplicate = None;
    for f in features.iter().map(|f| f.trim()) {
        if !seen.insert(f) && duplicate.is_none() {
            duplicate = Some(f.to_string());
        }
    }
    duplicate
}

/// `cceb3063e1...` -> `cceb306`: enough to recognize a commit, short enough to
/// fit in a status line.
pub fn short_rev(rev: &str) -> String {
    rev.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_spec(rev: Option<&str>) -> PinSpec {
        PinSpec {
            source: SourceFields {
                source: SourceKind::Git,
                url: Some("https://github.com/this-rs/this".into()),
                rev: rev.map(|r| r.to_string()),
                ..Default::default()
            },
            channel: Some(Channel::Stable),
            features: vec!["grpc".into()],
        }
    }

    #[test]
    fn a_git_declaration_needs_a_url() {
        let fields = SourceFields {
            source: SourceKind::Git,
            ..Default::default()
        };
        assert!(fields.to_source().unwrap_err().contains("url"));
    }

    #[test]
    fn a_crates_declaration_needs_a_version() {
        let fields = SourceFields {
            source: SourceKind::CratesIo,
            ..Default::default()
        };
        assert!(fields.to_source().unwrap_err().contains("version"));
    }

    #[test]
    fn a_path_declaration_needs_a_path() {
        let fields = SourceFields {
            source: SourceKind::Path,
            ..Default::default()
        };
        assert!(fields.to_source().unwrap_err().contains("path"));
    }

    #[test]
    fn a_channel_declaration_resolves_without_a_rev() {
        let source = git_spec(None).source().unwrap();
        assert_eq!(source.kind(), SourceKind::Git);
        assert!(source.rev().is_none());
        assert!(!source.is_reproducible());
    }

    #[test]
    fn a_pinned_declaration_is_reproducible() {
        let source = git_spec(Some("cceb3063e1aa")).source().unwrap();
        assert!(source.is_reproducible());
        assert_eq!(source.rev(), Some("cceb3063e1aa"));
    }

    #[test]
    fn a_local_path_is_never_reproducible() {
        let source = Source::Path {
            path: "../this".into(),
        };
        assert!(!source.is_reproducible());
    }

    #[test]
    fn features_are_normalized_deterministically() {
        let features = vec![
            "websocket".to_string(),
            "grpc".to_string(),
            " grpc ".to_string(),
            "".to_string(),
        ];
        assert_eq!(
            normalized_features(&features),
            vec!["grpc".to_string(), "websocket".to_string()]
        );
    }

    #[test]
    fn a_feature_asked_twice_is_reported() {
        let features = vec![
            "grpc".to_string(),
            "websocket".to_string(),
            "grpc".to_string(),
        ];
        assert_eq!(first_duplicate(&features), Some("grpc".to_string()));
    }

    #[test]
    fn the_declaration_reads_flat_yaml() {
        let yaml = r#"
source: git
url: https://github.com/this-rs/this
channel: canary
features:
  - grpc
"#;
        let spec: PinSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.source.source, SourceKind::Git);
        assert_eq!(spec.channel, Some(Channel::Canary));
        assert_eq!(spec.features, vec!["grpc".to_string()]);
    }

    #[test]
    fn a_declaration_without_a_channel_defaults_to_none() {
        let yaml = "source: crates-io\nversion: 0.0.9\n";
        let spec: PinSpec = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(spec.channel, None);
        assert!(spec.features.is_empty());
    }

    #[test]
    fn a_rev_is_shortened_for_display() {
        assert_eq!(short_rev("cceb3063e1aabbcc"), "cceb306");
        assert_eq!(short_rev("abc"), "abc");
    }
}
