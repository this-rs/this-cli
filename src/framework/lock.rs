//! What the project *resolved*, and the rules that keep the resolution honest.
//!
//! The declaration says what a project wants; the lock says what it got. The two
//! are separate on purpose: a channel can move without the proof of the commit
//! used moving with it. `this.yaml` is the intent, `.this/framework.lock` is the
//! evidence, and [`validate`] is what stops the evidence from lying.

use serde::{Deserialize, Serialize};

use super::pin::{Channel, PinSpec, Source, SourceFields, first_duplicate, normalized_features};
use super::{Diagnostic, Run, STALE_AFTER_MS};

/// One crate pinned by the lock.
///
/// `rev` is carried per crate because the monorepo may one day publish some
/// crates and keep others on git — and because seeing the rev repeated on every
/// line is what makes a mixture visible instead of silent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedCrate {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
}

impl LockedCrate {
    /// Written by the resolver (PIN-5); kept next to the struct it builds.
    #[allow(dead_code)]
    pub fn new(name: impl Into<String>, rev: Option<String>) -> Self {
        LockedCrate {
            name: name.into(),
            rev,
        }
    }
}

/// The `.this/framework.lock` file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lock {
    #[serde(flatten)]
    pub source: SourceFields,

    /// When this resolution happened, ISO-8601 UTC (`2026-11-04T21:59:02Z`).
    pub resolved_at: String,

    /// The channel that was followed when resolving, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<Channel>,

    /// Every crate taken from this source, with the commit it came from.
    #[serde(default)]
    pub crates: Vec<LockedCrate>,

    /// Features actually requested when this was written.
    #[serde(default)]
    pub features: Vec<String>,

    /// Digest of the API surface at this commit, once the digest exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_digest: Option<String>,

    /// Migrations applied to the project to reach this commit.
    #[serde(default)]
    pub migrations: Vec<String>,

    /// The commit this one replaced, so a move can be undone without a backup
    /// file: rolling back is upgrading to this commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_rev: Option<String>,
}

impl Lock {
    pub fn source(&self) -> Result<Source, String> {
        self.source.to_source()
    }

    /// How long ago this was resolved. `None` when the date cannot be read —
    /// and then freshness is not claimed, rather than guessed.
    pub fn age_ms(&self, now_ms: u64) -> Option<u64> {
        iso8601_ms(&self.resolved_at).map(|then| now_ms.saturating_sub(then))
    }

    pub fn from_yaml(yaml: &str) -> Result<Lock, serde_yaml::Error> {
        serde_yaml::from_str(yaml)
    }

    /// The counterpart of [`Lock::from_yaml`]; used by `framework resolve`.
    #[allow(dead_code)]
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }
}

/// Check a declaration against its lock.
///
/// Returns every problem found, most severe first in declaration order. An empty
/// vector means the lock tells the truth about the declaration.
pub fn validate(spec: &PinSpec, lock: Option<&Lock>, now_ms: u64, run: Run) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    let spec_source = match spec.source() {
        Ok(source) => source,
        Err(message) => {
            out.push(Diagnostic::error("spec-incomplete", message));
            return out;
        }
    };

    if let Source::Path { path } = &spec_source {
        match run {
            Run::Ci => out.push(Diagnostic::error(
                "path-pin-in-ci",
                format!(
                    "the framework is pinned to the local path `{path}`, which does not exist on another machine: CI cannot build this"
                ),
            )),
            Run::Local => out.push(Diagnostic::warn(
                "path-pin",
                format!("local path pin `{path}`: this project is not reproducible elsewhere"),
            )),
        }
    }

    let lock = match lock {
        Some(lock) => lock,
        None => {
            // Only an unresolved channel needs one: a published version is its
            // own answer, an explicit commit is its own answer, and a local path
            // is the resolution itself.
            if matches!(spec_source, Source::Git { rev: None, .. }) {
                out.push(Diagnostic::warn(
                    "lock-missing",
                    "no `.this/framework.lock`: the channel has not been resolved to a commit yet",
                ));
            }
            return out;
        }
    };

    let lock_source = match lock.source() {
        Ok(source) => source,
        Err(message) => {
            out.push(Diagnostic::error("lock-incomplete", message));
            return out;
        }
    };

    compare_sources(&spec_source, &lock_source, &mut out);

    // One commit for the whole monorepo. Cargo unifies a dependency by source:
    // two revs of the same repository produce two builds of the same crate and
    // types that do not match, so a mixture is a defect, not a preference.
    let revs: std::collections::BTreeSet<&str> = lock
        .crates
        .iter()
        .filter_map(|c| c.rev.as_deref())
        .collect();
    if revs.len() > 1 {
        let list: Vec<&str> = revs.into_iter().collect();
        out.push(Diagnostic::error(
            "multiple-revs",
            format!(
                "the lock carries {} different commits ({}): one commit per repository, otherwise Cargo builds the crate twice",
                list.len(),
                list.join(", ")
            ),
        ));
    }

    if let (Some(spec_channel), Some(lock_channel)) = (spec.channel, lock.channel)
        && spec_channel != lock_channel
    {
        out.push(Diagnostic::warn(
            "channel-drift",
            format!(
                "the project follows `{}` but the lock was resolved on `{}`",
                spec_channel.as_str(),
                lock_channel.as_str()
            ),
        ));
    }

    if let Some(duplicate) = first_duplicate(&lock.features) {
        out.push(Diagnostic::warn(
            "duplicate-feature",
            format!("feature `{duplicate}` is listed twice in the lock"),
        ));
    }

    let declared = normalized_features(&spec.features);
    let locked = normalized_features(&lock.features);
    if !declared.is_empty() && declared != locked {
        out.push(Diagnostic::warn(
            "features-drift",
            format!(
                "the declaration asks for [{}] but the lock records [{}]",
                declared.join(", "),
                locked.join(", ")
            ),
        ));
    }

    match lock.age_ms(now_ms) {
        Some(age) if age > STALE_AFTER_MS => out.push(Diagnostic::warn(
            "lock-stale",
            format!(
                "the lock was resolved {} days ago: the channel has likely moved since",
                age / 86_400_000
            ),
        )),
        Some(_) => {}
        None => out.push(Diagnostic::warn(
            "lock-date-unreadable",
            format!(
                "`resolved_at` = `{}` is not an ISO-8601 UTC date: freshness is unknown",
                lock.resolved_at
            ),
        )),
    }

    out
}

fn compare_sources(spec: &Source, lock: &Source, out: &mut Vec<Diagnostic>) {
    if spec.kind() != lock.kind() {
        out.push(Diagnostic::error(
            "lock-source-mismatch",
            format!(
                "the project declares `{}` but the lock says `{}`",
                spec.kind().as_str(),
                lock.kind().as_str()
            ),
        ));
        return;
    }

    match (spec, lock) {
        (Source::CratesIo { version: a }, Source::CratesIo { version: b }) => {
            if a != b {
                out.push(Diagnostic::error(
                    "lock-version-mismatch",
                    format!("the project declares version {a} but the lock says {b}"),
                ));
            }
        }
        (Source::Path { path: a }, Source::Path { path: b }) => {
            if a != b {
                out.push(Diagnostic::error(
                    "lock-path-mismatch",
                    format!("the project declares path `{a}` but the lock says `{b}`"),
                ));
            }
        }
        (
            Source::Git {
                url: spec_url,
                rev: spec_rev,
            },
            Source::Git {
                url: lock_url,
                rev: lock_rev,
            },
        ) => {
            if spec_url != lock_url {
                out.push(Diagnostic::error(
                    "lock-url-mismatch",
                    format!("the project declares `{spec_url}` but the lock says `{lock_url}`"),
                ));
            }
            match lock_rev {
                None => out.push(Diagnostic::error(
                    "lock-rev-missing",
                    "the lock names a git source without a commit: a branch is not a pin",
                )),
                Some(locked) => {
                    if let Some(declared) = spec_rev
                        && declared != locked
                    {
                        out.push(Diagnostic::error(
                            "lock-rev-mismatch",
                            format!(
                                "the project declares commit {} but the lock says {}",
                                &declared[..declared.len().min(12)],
                                &locked[..locked.len().min(12)]
                            ),
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}

/// The Cargo dependency line for one crate, or why it cannot be written.
///
/// A git source without a commit is refused: rendering it would pin the default
/// branch, which is the exact defect this whole module exists to remove.
pub fn dependency_line(
    alias: &str,
    package: &str,
    source: &Source,
    features: &[String],
) -> Result<String, String> {
    let mut parts = vec![format!("package = \"{package}\"")];

    match source {
        Source::CratesIo { version } => parts.push(format!("version = \"{version}\"")),
        Source::Path { path } => parts.push(format!("path = \"{path}\"")),
        Source::Git { url, rev } => {
            parts.push(format!("git = \"{url}\""));
            match rev {
                Some(rev) => parts.push(format!("rev = \"{rev}\"")),
                None => {
                    return Err(format!(
                        "`{alias}` points at {url} without a commit: resolve the channel before writing the manifest"
                    ));
                }
            }
        }
    }

    let features = normalized_features(features);
    if !features.is_empty() {
        let list: Vec<String> = features.iter().map(|f| format!("\"{f}\"")).collect();
        parts.push(format!("features = [{}]", list.join(", ")));
    }

    Ok(format!("{alias} = {{ {} }}", parts.join(", ")))
}

/// The `this` dependency of a generated project, resolved from the lock when
/// there is one, from the declaration otherwise.
pub fn this_dependency(spec: &PinSpec, lock: Option<&Lock>) -> Result<String, String> {
    let source = match lock {
        Some(lock) => lock.source()?,
        None => spec.source()?,
    };
    dependency_line("this", "this-rs", &source, &spec.features)
}

/// Parse `2026-11-04T21:59:02Z` (fractional seconds tolerated) into epoch ms.
pub fn iso8601_ms(text: &str) -> Option<u64> {
    let text = text.trim().strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;

    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;

    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: i64 = t.next()?.split('.').next()?.parse().ok()?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let ms = days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1_000;
    u64::try_from(ms).ok()
}

/// Format epoch milliseconds as `2026-11-04T21:59:02Z`, the inverse of
/// [`iso8601_ms`]. Whole seconds: a lock does not need a fraction it never reads.
pub fn iso8601_from_ms(ms: u64) -> String {
    let seconds = (ms / 1_000) as i64;
    let days = seconds.div_euclid(86_400);
    let remainder = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        remainder / 3_600,
        (remainder % 3_600) / 60,
        remainder % 60
    )
}

/// Howard Hinnant's `civil_from_days`, the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01, no dependency.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = (month as i64 + 9) % 12;
    let day_of_year = (153 * shifted_month + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::pin::{Channel, SourceKind};

    const NOW: u64 = 1_800_000_000_000; // 2027-01-15T08:00:00Z

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

    fn git_lock(rev: Option<&str>, crates: Vec<LockedCrate>) -> Lock {
        Lock {
            source: SourceFields {
                source: SourceKind::Git,
                url: Some("https://github.com/this-rs/this".into()),
                rev: rev.map(str::to_string),
                ..Default::default()
            },
            resolved_at: "2026-11-04T21:59:02Z".into(),
            channel: Some(Channel::Stable),
            crates,
            features: vec!["grpc".into()],
            api_digest: None,
            migrations: Vec::new(),
            previous_rev: None,
        }
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics.iter().map(|d| d.code).collect()
    }

    #[test]
    fn a_matching_git_pair_has_nothing_to_report() {
        let spec = git_spec(Some("abc123"), Some(Channel::Stable));
        let lock = git_lock(
            Some("abc123"),
            vec![LockedCrate::new("this-rs", Some("abc123".into()))],
        );
        // The lock above is dated 2026-11-04, `NOW` is ~70 days later: it is
        // stale, so only that warning may appear.
        assert_eq!(
            codes(&validate(&spec, Some(&lock), NOW, Run::Local)),
            vec!["lock-stale"]
        );
    }

    #[test]
    fn a_rev_that_disagrees_with_the_lock_is_an_error() {
        let spec = git_spec(Some("abc123"), None);
        let lock = git_lock(
            Some("def456"),
            vec![LockedCrate::new("this-rs", Some("def456".into()))],
        );
        let found = validate(&spec, Some(&lock), NOW, Run::Local);
        assert!(found.iter().any(|d| d.code == "lock-rev-mismatch"));
        assert!(found.iter().any(|d| d.is_error()));
    }

    #[test]
    fn a_lock_of_another_kind_is_an_error() {
        let spec = git_spec(Some("abc123"), None);
        let mut lock = git_lock(Some("abc123"), vec![]);
        lock.source = SourceFields {
            source: SourceKind::Path,
            path: Some("../this".into()),
            ..Default::default()
        };
        assert!(
            codes(&validate(&spec, Some(&lock), NOW, Run::Local)).contains(&"lock-source-mismatch")
        );
    }

    #[test]
    fn a_git_lock_without_a_commit_is_an_error() {
        let spec = git_spec(None, Some(Channel::Stable));
        let lock = git_lock(None, vec![]);
        assert!(
            codes(&validate(&spec, Some(&lock), NOW, Run::Local)).contains(&"lock-rev-missing")
        );
    }

    #[test]
    fn two_commits_in_one_lock_are_an_error() {
        let spec = git_spec(None, Some(Channel::Stable));
        let lock = git_lock(
            Some("abc123"),
            vec![
                LockedCrate::new("this-rs", Some("abc123".into())),
                LockedCrate::new("this-descriptor", Some("def456".into())),
            ],
        );
        let found = validate(&spec, Some(&lock), NOW, Run::Local);
        assert!(
            found
                .iter()
                .any(|d| d.code == "multiple-revs" && d.is_error())
        );
    }

    #[test]
    fn a_local_path_pin_is_refused_in_ci_and_only_warned_about_locally() {
        let spec = PinSpec {
            source: SourceFields {
                source: SourceKind::Path,
                path: Some("../this".into()),
                ..Default::default()
            },
            channel: None,
            features: vec![],
        };

        let local = validate(&spec, None, NOW, Run::Local);
        assert_eq!(codes(&local), vec!["path-pin"]);
        assert!(!local.iter().any(|d| d.is_error()));

        let ci = validate(&spec, None, NOW, Run::Ci);
        assert!(
            ci.iter()
                .any(|d| d.code == "path-pin-in-ci" && d.is_error())
        );
    }

    #[test]
    fn an_unresolved_channel_without_a_lock_is_reported() {
        let spec = git_spec(None, Some(Channel::Canary));
        assert_eq!(
            codes(&validate(&spec, None, NOW, Run::Local)),
            vec!["lock-missing"]
        );
    }

    #[test]
    fn a_published_version_needs_no_lock() {
        let spec = PinSpec::crates_io("0.0.9");
        assert!(validate(&spec, None, NOW, Run::Ci).is_empty());
    }

    #[test]
    fn a_fresh_lock_is_not_stale() {
        let spec = git_spec(Some("abc123"), Some(Channel::Stable));
        let lock = git_lock(Some("abc123"), vec![]);
        let just_resolved = iso8601_ms(&lock.resolved_at).unwrap() + 1_000;
        assert!(validate(&spec, Some(&lock), just_resolved, Run::Local).is_empty());
    }

    #[test]
    fn a_channel_that_moved_since_resolution_is_warned_about() {
        let spec = git_spec(None, Some(Channel::Next));
        let lock = git_lock(Some("abc123"), vec![]);
        let found = validate(&spec, Some(&lock), NOW, Run::Local);
        assert!(found.iter().any(|d| d.code == "channel-drift"));
        assert!(!found.iter().any(|d| d.is_error()));
    }

    #[test]
    fn features_that_drifted_are_warned_about() {
        let mut spec = git_spec(Some("abc123"), Some(Channel::Stable));
        spec.features = vec!["grpc".into(), "websocket".into()];
        let lock = git_lock(Some("abc123"), vec![]);
        let found = validate(&spec, Some(&lock), NOW, Run::Local);
        assert!(found.iter().any(|d| d.code == "features-drift"));
    }

    #[test]
    fn a_version_that_disagrees_with_the_lock_is_an_error() {
        let spec = PinSpec::crates_io("0.0.10");
        let lock = Lock {
            source: SourceFields {
                source: SourceKind::CratesIo,
                version: Some("0.0.9".into()),
                ..Default::default()
            },
            resolved_at: "2026-11-04T21:59:02Z".into(),
            channel: None,
            crates: vec![],
            features: vec![],
            api_digest: None,
            migrations: vec![],
            previous_rev: None,
        };
        assert!(
            codes(&validate(&spec, Some(&lock), NOW, Run::Local))
                .contains(&"lock-version-mismatch")
        );
    }

    #[test]
    fn a_published_dependency_line_is_exact() {
        let line = dependency_line(
            "this",
            "this-rs",
            &Source::CratesIo {
                version: "0.0.9".into(),
            },
            &["graphql".into(), "grpc".into()],
        )
        .unwrap();
        assert_eq!(
            line,
            "this = { package = \"this-rs\", version = \"0.0.9\", features = [\"graphql\", \"grpc\"] }"
        );
    }

    #[test]
    fn a_path_dependency_line_is_exact() {
        let line = dependency_line(
            "this",
            "this-rs",
            &Source::Path {
                path: "../this".into(),
            },
            &[],
        )
        .unwrap();
        assert_eq!(line, "this = { package = \"this-rs\", path = \"../this\" }");
    }

    #[test]
    fn a_git_dependency_line_carries_the_commit() {
        let line = dependency_line(
            "this",
            "this-rs",
            &Source::Git {
                url: "https://github.com/this-rs/this".into(),
                rev: Some("cceb306".into()),
            },
            &["grpc".into()],
        )
        .unwrap();
        assert_eq!(
            line,
            "this = { package = \"this-rs\", git = \"https://github.com/this-rs/this\", rev = \"cceb306\", features = [\"grpc\"] }"
        );
    }

    #[test]
    fn a_git_source_without_a_commit_cannot_be_written() {
        let err = dependency_line(
            "this",
            "this-rs",
            &Source::Git {
                url: "https://github.com/this-rs/this".into(),
                rev: None,
            },
            &[],
        )
        .unwrap_err();
        assert!(err.contains("without a commit"));
    }

    #[test]
    fn the_project_dependency_prefers_the_lock() {
        let spec = git_spec(None, Some(Channel::Stable));
        let lock = git_lock(Some("cceb306"), vec![]);
        let line = this_dependency(&spec, Some(&lock)).unwrap();
        assert!(line.contains("rev = \"cceb306\""));
    }

    #[test]
    fn a_lock_survives_a_yaml_round_trip() {
        let lock = git_lock(
            Some("abc123"),
            vec![LockedCrate::new("this-rs", Some("abc123".into()))],
        );
        let yaml = lock.to_yaml().unwrap();
        assert_eq!(Lock::from_yaml(&yaml).unwrap(), lock);
    }

    #[test]
    fn a_lock_of_another_url_is_an_error() {
        let spec = git_spec(Some("abc123"), None);
        let mut lock = git_lock(Some("abc123"), vec![]);
        lock.source.url = Some("https://gitlab.com/other/this".into());
        assert!(
            codes(&validate(&spec, Some(&lock), NOW, Run::Local)).contains(&"lock-url-mismatch")
        );
    }

    #[test]
    fn a_lock_with_another_path_is_an_error() {
        let spec = PinSpec {
            source: SourceFields {
                source: SourceKind::Path,
                path: Some("../this".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let lock = Lock {
            source: SourceFields {
                source: SourceKind::Path,
                path: Some("/elsewhere/this".into()),
                ..Default::default()
            },
            resolved_at: "2026-11-04T21:59:02Z".into(),
            channel: None,
            crates: vec![],
            features: vec![],
            api_digest: None,
            migrations: vec![],
            previous_rev: None,
        };
        assert!(
            codes(&validate(&spec, Some(&lock), NOW, Run::Local)).contains(&"lock-path-mismatch")
        );
    }

    #[test]
    fn an_incomplete_lock_is_an_error_rather_than_a_guess() {
        let spec = git_spec(Some("abc123"), None);
        let lock = Lock {
            source: SourceFields {
                source: SourceKind::Git,
                ..Default::default()
            },
            resolved_at: "2026-11-04T21:59:02Z".into(),
            channel: None,
            crates: vec![],
            features: vec![],
            api_digest: None,
            migrations: vec![],
            previous_rev: None,
        };
        assert!(codes(&validate(&spec, Some(&lock), NOW, Run::Local)).contains(&"lock-incomplete"));
    }

    #[test]
    fn a_feature_listed_twice_in_the_lock_is_warned_about() {
        let spec = git_spec(Some("abc123"), None);
        let mut lock = git_lock(Some("abc123"), vec![]);
        lock.features = vec!["grpc".into(), "grpc".into()];
        let found = validate(&spec, Some(&lock), NOW, Run::Local);
        assert!(found.iter().any(|d| d.code == "duplicate-feature"));
        assert!(!found.iter().any(|d| d.is_error()));
    }

    #[test]
    fn an_unreadable_date_makes_freshness_unknown_instead_of_stale() {
        let spec = git_spec(Some("abc123"), None);
        let mut lock = git_lock(Some("abc123"), vec![]);
        lock.resolved_at = "last tuesday".into();
        let found = validate(&spec, Some(&lock), NOW, Run::Local);
        assert!(found.iter().any(|d| d.code == "lock-date-unreadable"));
        assert!(!found.iter().any(|d| d.code == "lock-stale"));
    }

    #[test]
    fn an_unreadable_date_has_no_age_to_report() {
        let lock = Lock {
            resolved_at: "not a date".into(),
            ..git_lock(Some("abc123"), vec![])
        };
        assert!(lock.age_ms(NOW).is_none());
    }

    #[test]
    fn an_iso_date_is_read_back_to_the_millisecond() {
        assert_eq!(iso8601_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso8601_ms("2026-11-04T21:59:02Z"), Some(1_793_829_542_000));
        assert_eq!(
            iso8601_ms("2026-11-04T21:59:02.750Z"),
            Some(1_793_829_542_000)
        );
    }

    #[test]
    fn a_date_that_is_not_iso_is_refused_rather_than_guessed() {
        assert!(iso8601_ms("yesterday").is_none());
        assert!(iso8601_ms("2026-13-01T00:00:00Z").is_none());
        assert!(iso8601_ms("2026-11-04 21:59:02").is_none());
    }
}
