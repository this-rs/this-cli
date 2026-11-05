//! What the framework broke, and whether *this* project is affected.
//!
//! Moving a pin is only safe if the project survives the move. Between two
//! commits a symbol is renamed, a rule changes meaning; a migration says so, and
//! the point of [`detect`] is to answer one question about one project: does
//! this concern me?
//!
//! Two kinds of migration, and the difference matters:
//!
//! - [`Detect::Mentions`] — the project shows the old symbol, so it is
//!   concerned. Decidable from the sources.
//! - [`Detect::Always`] — the change is about an **absence** (a rule that is now
//!   required but was optional). No text can prove it, so it is shown every
//!   time rather than silently skipped: a false negative is worse than a
//!   redundant line.
//!
//! No migration carries an automatic repair yet. The registry says what is
//! about to break; mending stays a human act, and `upgrade` says so instead of
//! pretending.

use std::path::Path;

/// A change that can break a project built against an older commit.
pub struct Migration {
    /// Stable identifier, recorded once a migration has been dealt with.
    pub id: &'static str,
    /// One line a user can act on.
    pub summary: &'static str,
    pub detect: Detect,
}

/// How to tell whether a project is concerned.
pub enum Detect {
    /// The project mentions one of these symbols.
    Mentions(&'static [&'static str]),
    /// Cannot be decided from the text: shown for every move.
    Always,
}

/// The migrations this CLI knows about.
///
/// Compiled in, because a CLI ships alongside the framework it was built
/// against. Each entry is a change that **breaks or silently changes** a
/// project, verified against the source — not a guess, and not an addition.
///
/// The distinction is the whole point, and the first version of this registry
/// got it wrong: it announced that `Session` had *become* `Capabilities`. Both
/// exist (`crates/this-descriptor/src/capabilities.rs:152` and `:378`, no
/// `#[deprecated]` anywhere), `Capabilities` only adds a layer on top of
/// `Session`, and nothing breaks. A registry that names a break where there is
/// an addition is worse than an empty one: it sends people to change code that
/// works.
pub const REGISTRY: &[Migration] = &[
    Migration {
        id: "0001",
        summary: "deleting a record a `Ref` points at no longer goes through silently: `on_delete` defaults to `restrict`, `cascade` opts in",
        // The field has a default, so nothing fails to compile: what changes is
        // the answer to a deletion. No text can prove a project relies on it.
        detect: Detect::Always,
    },
    Migration {
        id: "0002",
        summary: "link events are published by the seam `with_event_bus` installs: a host assembled by setting `event_bus` directly publishes none",
        // Qualified forms only. A bare `Session` matched ScyllaDB's, whose
        // stores template writes `scylla::client::session::Session` — a warning
        // that fires on an homonym is worse than no warning at all.
        detect: Detect::Mentions(&["with_event_bus", "event_bus"]),
    },
];

/// Which migrations concern a project that reads `text`.
pub fn detect<'a>(text: &str, migrations: &'a [Migration]) -> Vec<&'a Migration> {
    migrations
        .iter()
        .filter(|migration| match &migration.detect {
            Detect::Always => true,
            Detect::Mentions(symbols) => symbols.iter().any(|symbol| mentioned(text, symbol)),
        })
        .collect()
}

/// The symbol a mention matched, for the report.
pub fn matched<'a>(migration: &Migration, text: &str) -> Option<&'a str> {
    match &migration.detect {
        Detect::Always => None,
        Detect::Mentions(symbols) => symbols
            .iter()
            .find(|symbol| mentioned(text, symbol))
            .copied(),
    }
}

/// Whether `symbol` appears as a word, not as part of a longer identifier.
///
/// `Session` must not match `SessionStore`, or every project would look
/// concerned by every rename.
pub fn mentioned(text: &str, symbol: &str) -> bool {
    let mut from = 0;
    while let Some(offset) = text[from..].find(symbol) {
        let start = from + offset;
        let end = start + symbol.len();

        let before_ok = match text[..start].chars().next_back() {
            Some(c) => !c.is_alphanumeric() && c != '_',
            None => true,
        };
        let after_ok = match text[end..].chars().next() {
            Some(c) => !c.is_alphanumeric() && c != '_',
            None => true,
        };

        if before_ok && after_ok {
            return true;
        }
        from = end.max(from + 1);
    }
    false
}

/// Every source and configuration file of a crate, concatenated.
///
/// Concatenated rather than parsed: the question is "is this symbol written
/// somewhere in this project", and one big haystack answers it without a parser
/// for each file type.
pub fn scan_sources(crate_dir: &Path) -> Result<String, String> {
    let mut out = String::new();
    for folder in ["src", "config"] {
        collect(&crate_dir.join(folder), &mut out, 0)?;
    }
    Ok(out)
}

fn collect(dir: &Path, out: &mut String, depth: usize) -> Result<(), String> {
    // A bounded walk: a symlink loop must not become an infinite read.
    if depth > 16 {
        return Ok(());
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        // No directory is not an error: a crate may have no config/.
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out, depth + 1)?;
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs" | "yaml" | "yml")
        ) && let Ok(text) = std::fs::read_to_string(&path)
        {
            out.push_str(&text);
            out.push('\n');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mentions_migration(symbols: &'static [&'static str]) -> Migration {
        Migration {
            id: "test",
            summary: "test",
            detect: Detect::Mentions(symbols),
        }
    }

    #[test]
    fn a_symbol_is_matched_as_a_word() {
        assert!(mentioned("fn f(s: Session)", "Session"));
        assert!(mentioned("Session", "Session"));
        assert!(mentioned("let s = Session::anonymous();", "Session"));
    }

    #[test]
    fn a_type_name_from_another_crate_is_not_a_match() {
        // Measured: the generated stores template mentions
        // `scylla::client::session::Session`, which is not this-rs's Session.
        let registry = [mentions_migration(&["Session::anonymous"])];
        let scylla = "pub fn new(session: Arc<scylla::client::session::Session>) {}";
        assert!(detect(scylla, &registry).is_empty());
    }

    #[test]
    fn a_longer_identifier_is_not_a_match() {
        assert!(!mentioned("struct SessionStore;", "Session"));
        assert!(!mentioned("my_Session", "Session"));
        assert!(!mentioned("Sessions", "Session"));
    }

    #[test]
    fn a_path_qualified_symbol_matches() {
        assert!(mentioned(
            "PolicyExpr::permissions()",
            "PolicyExpr::permissions"
        ));
        assert!(!mentioned(
            "PolicyExpr::permissions_all()",
            "PolicyExpr::permissions"
        ));
    }

    #[test]
    fn only_the_migrations_that_mention_something_are_kept() {
        let registry = [mentions_migration(&["Session"])];
        assert_eq!(detect("fn f(s: Session)", &registry).len(), 1);
        assert!(detect("fn f(s: String)", &registry).is_empty());
    }

    #[test]
    fn an_undecidable_migration_is_always_reported() {
        let registry = [Migration {
            id: "always",
            summary: "check it yourself",
            detect: Detect::Always,
        }];
        assert_eq!(detect("nothing here", &registry).len(), 1);
    }

    #[test]
    fn the_registry_tells_which_symbol_matched() {
        let registry = [mentions_migration(&["Session", "event_bus"])];
        assert_eq!(
            matched(&registry[0], "let b = event_bus;"),
            Some("event_bus")
        );
        assert_eq!(matched(&registry[0], "let s = Session;"), Some("Session"));
        assert_eq!(matched(&registry[0], "nothing"), None);
    }

    #[test]
    fn the_shipped_registry_is_detectable() {
        // The real registry, on a text that mentions nothing: only the
        // undecidable one is reported.
        let found = detect("fn main() {}", REGISTRY);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "0001");

        // A mention pulls in the one that can be decided from the text.
        let found = detect("host.with_event_bus(1024);", REGISTRY);
        assert!(found.iter().any(|m| m.id == "0002"));

        // And the claim the first version made — that `Session` had become
        // `Capabilities` — must not send anybody anywhere: it is not a break.
        let found = detect("let s = Session::anonymous();", REGISTRY);
        assert!(
            !found.iter().any(|m| m.id == "0002"),
            "an addition is not a migration"
        );
    }

    #[test]
    fn the_sources_of_a_crate_are_read_across_folders_and_depth() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/entities")).unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() { let s = Session; }",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/entities/product.rs"),
            "struct Product;",
        )
        .unwrap();
        std::fs::write(dir.path().join("config/links.yaml"), "product: {}\n").unwrap();
        std::fs::write(dir.path().join("src/notes.txt"), "Session").unwrap();

        let text = scan_sources(dir.path()).unwrap();

        assert!(text.contains("Session"), "rust sources are read");
        assert!(text.contains("Product"), "nested files are read");
        assert!(text.contains("product: {}"), "yaml is read");
        assert!(
            !text.contains("notes.txt") && !text.contains("Sessionnotes"),
            "txt is left out"
        );
    }

    #[test]
    fn a_crate_without_sources_is_empty_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(scan_sources(dir.path()).unwrap(), "");
    }
}
