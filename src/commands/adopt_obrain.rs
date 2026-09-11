//! `this adopt obrain` — brancher this-rs + obrain + IAM sur un projet **existant**.
//!
//! # Ce que la commande fait, et ce qu'elle refuse de faire
//!
//! Un projet qui tourne déjà a ses politiques, son fournisseur
//! d'authentification, ses entités et sa base. Cette commande **prépare** le
//! terrain :
//!
//! 1. elle active la feature `obrain` sur la dépendance `this-rs` (édition TOML
//!    qui préserve la mise en forme du fichier) ;
//! 2. si le projet porte les marqueurs posés par `this init`
//!    (`[this:obrain_store]`), elle insère la ligne de branchement à l'endroit
//!    prévu — sinon elle **affiche** le fragment, sans deviner où il va ;
//! 3. elle rapporte l'état réel : la dépendance était-elle là, le branchement
//!    est-il déjà présent, quelles politiques ajouter, et dans quel ordre
//!    adopter (observation d'abord).
//!
//! Elle **n'écrit aucune configuration d'authentification**. Écrire un
//! `config/auth.yaml` dans un projet existant pourrait désarmer son
//! authentification (un `provider: none` écrit par erreur, par exemple), et
//! décider de sa politique d'accès n'est pas le travail d'une commande de
//! scaffolding. Elle affiche les politiques à ajouter, elle ne les impose pas.
//!
//! Idempotente : la relancer ne change rien.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use colored::Colorize;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value, value};

use super::AdoptObrainArgs;
use crate::utils::file_writer::FileWriter;
use crate::utils::{markers, output, project};

/// Le marqueur que `this init` pose là où le store doit être branché.
const STORE_MARKER: &str = "[this:obrain_store]";

/// La ligne insérée après le marqueur (quand il existe).
const STORE_LINE: &str =
    ".with_obrain_store(store)  // relief, identité, IAM, application des politiques";

/// La feature à activer.
const FEATURE: &str = "obrain";

/// Le nom de la dépendance du framework.
const DEP: &str = "this-rs";

/// Ce que la commande a fait — rendu aussi par les tests.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AdoptionReport {
    /// La feature a-t-elle été ajoutée (faux = déjà présente) ?
    pub feature_added: bool,
    /// Le fichier `Cargo.toml` a-t-il été modifié ?
    pub manifest_updated: bool,
    /// Où la feature a été écrite (`dependencies` ou `workspace.dependencies`).
    pub manifest_section: String,
    /// Le branchement est-il déjà présent dans le code ?
    pub wiring_present: bool,
    /// La ligne de branchement a-t-elle été insérée (marqueur trouvé) ?
    pub wiring_inserted: bool,
    /// Le fragment à recopier, quand il n'a pas pu être inséré.
    pub wiring_snippet: Option<String>,
}

/// Point d'entrée de la commande.
pub fn run(args: AdoptObrainArgs, writer: &dyn FileWriter) -> Result<()> {
    let root = match args.path {
        Some(path) => path,
        None => project::detect_project_root_from(
            &std::env::current_dir().context("répertoire courant")?,
        )?,
    };

    output::print_step("this adopt obrain");
    println!("  projet : {}", root.display());
    println!();

    let report = adopt(&root, writer, args.report_only)?;
    print_report(&report, args.report_only);

    if writer.is_dry_run() {
        println!("  {}", "mode --dry-run : rien n'a été écrit".yellow());
    }
    Ok(())
}

/// Le travail, séparé de l'affichage — et donc testable.
pub fn adopt(root: &Path, writer: &dyn FileWriter, report_only: bool) -> Result<AdoptionReport> {
    let manifest = root.join("Cargo.toml");
    if !manifest.exists() {
        bail!(
            "aucun Cargo.toml dans {} — `this adopt obrain` s'adresse à un projet Rust existant",
            root.display()
        );
    }

    let original = std::fs::read_to_string(&manifest)
        .with_context(|| format!("lecture de {}", manifest.display()))?;
    let mut doc: DocumentMut = original
        .parse()
        .with_context(|| format!("{} n'est pas un TOML valide", manifest.display()))?;

    // ── 1. La feature obrain sur la dépendance this-rs ──────────────────
    let (section, added) = enable_feature(&mut doc)?;

    let updated = doc.to_string();
    let manifest_updated = updated != original;
    if manifest_updated {
        writer.update_file(&manifest, &original, &updated)?;
    }

    // ── 2. Le branchement, s'il y a un marqueur ─────────────────────────
    let mut report = AdoptionReport {
        feature_added: added,
        manifest_updated,
        manifest_section: section,
        ..Default::default()
    };

    let wiring_present = source_contains(root, "with_obrain_store")?;
    report.wiring_present = wiring_present;

    if !wiring_present {
        match marker_file(root)? {
            Some((path, content)) => {
                let updated = markers::insert_after_marker(&content, STORE_MARKER, STORE_LINE)?;
                writer.update_file(&path, &content, &updated)?;
                report.wiring_inserted = true;
            }
            None => report.wiring_snippet = Some(wiring_snippet(report_only)),
        }
    }

    Ok(report)
}

/// Active `features = ["obrain"]` sur `this-rs`, où qu'il soit déclaré.
///
/// Rend `(section, ajoutée)`. Trois formes sont reconnues, parce qu'un projet
/// réel en utilise une des trois :
///
/// * `this-rs = "0.0.9"` (chaîne) → devient une table en ligne avec la feature ;
/// * `this-rs = { path = "…", features = […] }` (table en ligne) ;
/// * `[dependencies.this-rs]` (table complète).
///
/// Et deux emplacements : `[dependencies]`, ou `[workspace.dependencies]` quand
/// le membre déclare `this-rs.workspace = true` — auquel cas c'est **là** qu'il
/// faut ajouter la feature, sinon le membre ne la verrait pas.
fn enable_feature(doc: &mut DocumentMut) -> Result<(String, bool)> {
    // Le membre délègue-t-il au workspace ?
    let delegated = doc
        .get("dependencies")
        .and_then(|d| d.get(DEP))
        .and_then(|item| item.get("workspace"))
        .and_then(|w| w.as_bool())
        .unwrap_or(false);

    let section = if delegated {
        "[workspace.dependencies]".to_string()
    } else {
        "[dependencies]".to_string()
    };

    // Navigation explicite : selon le cas, la table est à deux ou trois niveaux.
    // (Une première version construisait un chemin de longueur variable puis
    // l'indexait à position fixe — elle paniquait sur le cas le plus courant.)
    let item = if delegated {
        doc.get_mut("workspace")
            .and_then(|workspace| workspace.get_mut("dependencies"))
            .and_then(|deps| deps.get_mut(DEP))
    } else {
        doc.get_mut("dependencies")
            .and_then(|deps| deps.get_mut(DEP))
    };

    let Some(item) = item else {
        // Le projet n'utilise pas encore le framework : on ne devine ni version
        // ni chemin, on dit exactement quoi écrire.
        bail!(
            "`{DEP}` n'est pas une dépendance de ce projet ({section}). Ajoutez-la d'abord, \
             par exemple :\n\n    {DEP} = {{ version = \"0.0.9\", features = [\"{FEATURE}\"] }}\n\n\
             puis relancez `this adopt obrain`."
        );
    };

    match item {
        // `this-rs = "0.0.9"` → table en ligne avec la feature.
        Item::Value(Value::String(s)) => {
            let version = s.value().to_string();
            let mut table = InlineTable::new();
            table.insert("version", Value::from(version));
            let mut features = Array::new();
            features.push(FEATURE);
            table.insert("features", Value::Array(features));
            *item = Item::Value(Value::InlineTable(table));
            Ok((section, true))
        }
        // `this-rs = { … }`
        Item::Value(Value::InlineTable(table)) => Ok((section, push_feature_in_inline(table))),
        // `[dependencies.this-rs]`
        Item::Table(table) => Ok((section, push_feature_in_table(table))),
        _ => bail!(
            "forme de dépendance `{DEP}` non reconnue — ajoutez la feature {FEATURE} à la main"
        ),
    }
}

/// Ajoute la feature à une table en ligne. Rend `true` si elle a été ajoutée.
fn push_feature_in_inline(table: &mut InlineTable) -> bool {
    match table.get_mut("features") {
        Some(Value::Array(array)) => push_unique(array),
        Some(other) => {
            let mut array = Array::new();
            array.push(FEATURE);
            *other = Value::Array(array);
            true
        }
        None => {
            let mut array = Array::new();
            array.push(FEATURE);
            table.insert("features", Value::Array(array));
            true
        }
    }
}

/// Ajoute la feature à une table complète. Rend `true` si elle a été ajoutée.
fn push_feature_in_table(table: &mut Table) -> bool {
    match table.get_mut("features") {
        Some(Item::Value(Value::Array(array))) => push_unique(array),
        Some(item) => {
            let mut array = Array::new();
            array.push(FEATURE);
            *item = Item::Value(Value::Array(array));
            true
        }
        None => {
            let mut array = Array::new();
            array.push(FEATURE);
            table.insert("features", value(array));
            true
        }
    }
}

/// Ajoute la feature si elle est absente. Rend `true` si le tableau a changé.
fn push_unique(array: &mut Array) -> bool {
    if array.iter().any(|v| v.as_str() == Some(FEATURE)) {
        return false;
    }
    array.push(FEATURE);
    true
}

/// Le premier fichier source qui porte le marqueur de branchement.
fn marker_file(root: &Path) -> Result<Option<(PathBuf, String)>> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_rust_files(&root.join("src"), &mut candidates, 0)?;
    for path in candidates {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if content.contains(STORE_MARKER) {
            return Ok(Some((path, content)));
        }
    }
    Ok(None)
}

/// Le code du projet contient-il déjà ce fragment ?
fn source_contains(root: &Path, needle: &str) -> Result<bool> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_rust_files(&root.join("src"), &mut candidates, 0)?;
    Ok(candidates.iter().any(|path| {
        std::fs::read_to_string(path)
            .map(|content| content.contains(needle))
            .unwrap_or(false)
    }))
}

/// Parcourt `*.rs` sous un répertoire (profondeur bornée : on ne descend pas
/// dans un dépôt de dépendances).
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > 6 || !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("lecture de {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name == "target" || name.starts_with('.') {
                continue;
            }
            collect_rust_files(&path, out, depth + 1)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// Le fragment à recopier quand aucun marqueur n'existe.
fn wiring_snippet(report_only: bool) -> String {
    let mut snippet = String::from(
        "ServerBuilder::new()\n    .register_module(module)?\n    .with_obrain_store(store)",
    );
    if report_only {
        snippet.push_str(
            "\n    // Première étape : OBSERVER. Les refus sont calculés, journalisés\n    \
             // et publiés (en-tête `x-this-rs-authz`), mais rien n'est bloqué.\n    \
             .authorization_report_only()",
        );
    }
    snippet.push_str(
        "\n    // Si vos identités ne sont pas (user_id, tenant_id) :\n    \
         // .with_principal_mapping(|ctx| …)\n    .build_host()?",
    );
    snippet
}

/// L'affichage — ce que la commande a fait, et ce qui reste à faire.
fn print_report(report: &AdoptionReport, report_only: bool) {
    println!("  {}", "1. Dépendance".bold());
    if report.feature_added {
        println!(
            "     feature « {} » activée sur `{}` dans {}",
            FEATURE, DEP, report.manifest_section
        );
    } else if report.manifest_updated {
        println!("     Cargo.toml mis à jour ({})", report.manifest_section);
    } else {
        println!(
            "     déjà en place : `{}` porte la feature « {} » ({})",
            DEP, FEATURE, report.manifest_section
        );
    }
    println!();

    println!("  {}", "2. Branchement du store".bold());
    if report.wiring_present {
        println!("     déjà présent dans le code (`.with_obrain_store(…)`)");
    } else if report.wiring_inserted {
        println!("     ligne insérée après le marqueur `{STORE_MARKER}`");
    } else {
        println!("     aucun marqueur `{STORE_MARKER}` : à recopier où vous construisez l'hôte —");
        println!();
        for line in report.wiring_snippet.as_deref().unwrap_or("").lines() {
            println!("       {}", line);
        }
    }
    println!();

    println!("  {}", "3. Ce qui suit (dans cet ordre)".bold());
    println!(
        "     a. {} l'observation : {}",
        "activez d'abord".bold(),
        if report_only {
            "c'est ce que fait le fragment ci-dessus".to_string()
        } else {
            "`.authorization_report_only()` — les refus sont observés, rien n'est bloqué"
                .to_string()
        }
    );
    println!(
        "     b. laissez tourner, puis lisez les journaux : {}",
        "« autorisation : refus OBSERVÉ »".italic()
    );
    println!("     c. corrigez les politiques qui refusent à tort, puis retirez l'observation");
    println!(
        "     d. si vos identités ne sont pas `(user_id, tenant_id)` : {}",
        "`.with_principal_mapping(… )`".bold()
    );
    println!("     e. vérifiez qu'un second écrivain est refusé (le bail est armé par défaut)");
    println!();
    println!(
        "  {} {}",
        "Guide complet :".bold(),
        "this-rs/docs/adoption-projet-existant.md (dans le dépôt du framework)".cyan()
    );
    println!(
        "  {} cette commande n'écrit AUCUNE configuration d'authentification : vos politiques",
        "Note :".bold()
    );
    println!("         et votre fournisseur restent les vôtres. Elle prépare, elle ne décide pas.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::file_writer::RealWriter;

    fn project(manifest: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("répertoire temporaire");
        std::fs::write(dir.path().join("Cargo.toml"), manifest).expect("Cargo.toml");
        std::fs::create_dir_all(dir.path().join("src")).expect("src");
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() { let host = ServerBuilder::new().build_host(); }\n",
        )
        .expect("main.rs");
        dir
    }

    fn manifest(dir: &tempfile::TempDir) -> String {
        std::fs::read_to_string(dir.path().join("Cargo.toml")).expect("lecture")
    }

    #[test]
    fn a_string_dependency_becomes_an_inline_table_with_the_feature() {
        let dir = project(
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n[dependencies]\nthis-rs = \"0.0.9\"\n",
        );
        let report = adopt(dir.path(), &RealWriter, false).expect("adoption");

        assert!(report.feature_added);
        assert_eq!(report.manifest_section, "[dependencies]");
        let after = manifest(&dir);
        assert!(after.contains("this-rs"), "{after}");
        assert!(
            after.contains("obrain"),
            "la feature doit être écrite : {after}"
        );
        assert!(
            after.contains("0.0.9"),
            "la version doit être préservée : {after}"
        );
    }

    #[test]
    fn an_inline_table_keeps_its_other_keys() {
        let dir =
            project("[dependencies]\nthis-rs = { path = \"../this\", default-features = false }\n");
        adopt(dir.path(), &RealWriter, false).expect("adoption");

        let after = manifest(&dir);
        assert!(after.contains("path"), "{after}");
        assert!(
            after.contains("default-features"),
            "les clés existantes sont préservées : {after}"
        );
        assert!(after.contains("obrain"), "{after}");
    }

    #[test]
    fn a_full_table_dependency_is_handled_too() {
        let dir = project("[dependencies.this-rs]\npath = \"../this\"\nfeatures = [\"wami\"]\n");
        let report = adopt(dir.path(), &RealWriter, false).expect("adoption");

        assert!(report.feature_added);
        let after = manifest(&dir);
        assert!(
            after.contains("wami"),
            "les features existantes sont préservées : {after}"
        );
        assert!(after.contains("obrain"), "{after}");
    }

    #[test]
    fn a_workspace_dependency_is_edited_where_the_feature_belongs() {
        let dir = project(
            "[package]\nname = \"member\"\n\n[dependencies]\nthis-rs.workspace = true\n\n[workspace.dependencies]\nthis-rs = \"0.0.9\"\n",
        );
        let report = adopt(dir.path(), &RealWriter, false).expect("adoption");

        assert_eq!(
            report.manifest_section, "[workspace.dependencies]",
            "la feature doit aller là où la dépendance est réellement déclarée"
        );
        let after = manifest(&dir);
        assert!(after.contains("obrain"), "{after}");
    }

    #[test]
    fn running_twice_changes_nothing() {
        let dir = project("[dependencies]\nthis-rs = \"0.0.9\"\n");
        let first = adopt(dir.path(), &RealWriter, false).expect("première");
        let after_first = manifest(&dir);

        let second = adopt(dir.path(), &RealWriter, false).expect("seconde");
        assert!(
            !second.feature_added,
            "la seconde exécution ne doit rien ajouter"
        );
        assert!(
            !second.manifest_updated,
            "et ne doit pas réécrire le fichier"
        );
        assert_eq!(manifest(&dir), after_first, "idempotence");
        assert!(first.feature_added);
    }

    #[test]
    fn a_missing_dependency_fails_with_the_exact_line_to_add() {
        let dir = project("[dependencies]\nserde = \"1\"\n");
        let err = adopt(dir.path(), &RealWriter, false).expect_err("doit échouer");
        let message = format!("{err:#}");

        assert!(message.contains("this-rs"), "{message}");
        assert!(
            message.contains("obrain"),
            "le message doit donner la ligne exacte : {message}"
        );
        assert_eq!(
            manifest(&dir),
            "[dependencies]\nserde = \"1\"\n",
            "un échec ne doit rien modifier"
        );
    }

    #[test]
    fn the_wiring_is_inserted_after_the_marker_when_it_exists() {
        let dir = project("[dependencies]\nthis-rs = \"0.0.9\"\n");
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() {\n    let host = ServerBuilder::new()\n        // [this:obrain_store]\n        .build_host();\n}\n",
        )
        .expect("main.rs");

        let report = adopt(dir.path(), &RealWriter, false).expect("adoption");
        assert!(report.wiring_inserted);
        assert!(report.wiring_snippet.is_none());

        let source = std::fs::read_to_string(dir.path().join("src/main.rs")).expect("lecture");
        assert!(source.contains("with_obrain_store"), "{source}");
    }

    #[test]
    fn an_existing_wiring_is_recognised_and_not_duplicated() {
        let dir = project("[dependencies]\nthis-rs = \"0.0.9\"\n");
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() { ServerBuilder::new().with_obrain_store(store).build_host(); }\n",
        )
        .expect("main.rs");

        let report = adopt(dir.path(), &RealWriter, false).expect("adoption");
        assert!(report.wiring_present);
        assert!(!report.wiring_inserted);
        assert!(report.wiring_snippet.is_none());
    }

    #[test]
    fn the_snippet_offers_observation_first() {
        let with = wiring_snippet(true);
        assert!(with.contains("authorization_report_only"), "{with}");
        assert!(with.contains("with_obrain_store"), "{with}");
        assert!(
            with.contains("with_principal_mapping"),
            "le mappage doit être montré : {with}"
        );

        let without = wiring_snippet(false);
        assert!(
            !without.contains("authorization_report_only"),
            "sans --report-only, le fragment ne doit pas l'imposer : {without}"
        );
    }

    #[test]
    fn no_authentication_configuration_is_ever_written() {
        let dir = project("[dependencies]\nthis-rs = \"0.0.9\"\n");
        adopt(dir.path(), &RealWriter, false).expect("adoption");

        assert!(
            !dir.path().join("config/auth.yaml").exists(),
            "la commande ne doit PAS créer de configuration d'authentification : \
             elle pourrait désarmer celle du projet"
        );
    }
}
