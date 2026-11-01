# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.5] — en préparation

Cette version n'a jamais été publiée : `0.0.4` reste la dernière sur crates.io. Le numéro est
retenu depuis le 30/10/2026, release alors différée. Trente commits séparent les deux.

### Added

- **`this descriptor` : la CLI lit et écrit le `ModuleDescriptor`.** `init` matérialise
  `descriptor/module.yaml` et `descriptor.lock`, `check` valide, `export` rend le descripteur (JSON,
  YAML, `.proto`, SDL), `lock` fige les tags proto. La CLI devient le second écrivain d'un contrat
  qui n'en avait qu'un (le serveur), au lieu de le redériver.
- **`this add module`**, et l'outil MCP correspondant : le catalogue d'outils expose `descriptor` et
  `add_module`, donc un agent peut décrire un module sans passer par le shell.
- **`this init --auth` et `--cognitive`** : le squelette peut naître avec l'authentification ou le
  moteur cognitif déjà branchés, plutôt qu'à ajouter après coup.
- **`this init` écrit la configuration d'exposition** — CORS, domaines, Caddy, DNS — **inerte par
  défaut**. Écrire une politique d'exposition sans l'armer évite qu'un `init` ouvre une surface que
  personne n'a demandée.
- **`this adopt obrain`**, exposé aussi comme outil MCP : prépare un projet existant à obrain et à
  l'IAM, en éditant son `Cargo.toml` et en insérant l'appel par marqueur. La commande **refuse
  d'écrire une configuration d'authentification** — un `auth.yaml` posé dans un projet qui tourne
  pourrait désarmer son authentification.
- **Chaîne front vite-rs** : `this dev` sert le SPA depuis le processus API (rechargement, pont
  `/__dev`), sans serveur Node. `--this-path` scaffolde contre un checkout local.

### Changed

- **Le `Module::descriptor()` engendré applique `descriptor.lock`** : un module compilé est servi
  avec ses tags proto stables au lieu de les perdre à l'exécution.
- **Les gabarits de handler lisent `ListQuery`** au lieu de `QueryParams::filter_value` : un filtre
  mal formé est une erreur typée, plus un JSON ignoré en silence.
- **Les gabarits suivent la nouvelle API du builder** ; les projets engendrés disent désormais que
  **l'autorisation est ARMÉE** par défaut, au lieu de laisser croire qu'elle est optionnelle.

### Fixed

- **`this descriptor check` vérifie ce que le serveur compose** : les arguments de règle sont gardés
  verbatim et les règles de recouvrement conservées — la CLI validait un descripteur que le serveur
  refusait.
- **`links.yaml` et `events.yaml` conservent leurs clés inconnues** à la réécriture. Une commande
  d'édition ne doit pas supprimer une configuration qu'elle ne comprend pas.
- **Windows** : le lien vers `this-rs/this` est posé en *junction* (un `ln -s` n'y fonctionne pas).

### Testing

- Couverture des chemins `descriptor` (`--fields`, lecteur de code, composition du lock, `--output`,
  erreurs), de `doctor` (politiques de liens, dérive des manifestes, contrôles d'identité) et de
  l'outil MCP du descripteur.
- Les tests d'intégration tournent contre le binaire que cargo vient de construire, pas contre un
  `this` du `PATH`.

### Internal

- `this-descriptor` suivi en `0.0.1` (dépendance `version + path` : la CLI ne tire pas le framework).
- Clippy : 50 `&` redondants retirés des `format!` (Rust 1.97).
- CI : pas de build pour un changement purement documentaire (`*.md`, `docs/**`, `*.mmd`).
