# Descripteur de module dans this-cli

Le descripteur de module (`this_descriptor::ModuleDescriptor`, RFC `module-descriptor` du dépôt this)
décrit une fois les entités, liens, erreurs, messages et permissions d'un module. this-cli le lit et
l'écrit ; il n'en recopie plus les types (dépendance `this-descriptor`, chemin `../this/crates/this-descriptor`).

## Activer

```sh
this descriptor init        # crée descriptor/module.yaml et Module::descriptor() dans src/module.rs
```

Sans `descriptor/module.yaml`, toutes les commandes se comportent comme avant.

| Fichier | Propriétaire | Rôle |
|---|---|---|
| `descriptor/module.yaml` | vous (+ `this add …`) | identité, `requires`, liens, erreurs, clés i18n, permissions par entité |
| `src/entities/*/model.rs` | la macro | champs, types, pluriel, règles de champ (jamais `fields:` dans module.yaml) |
| `descriptor/descriptor.lock` | `this descriptor lock` | tag proto par champ, jamais déplacé ni réutilisé |
| `descriptor/module.descriptor.json` | `this descriptor export` | descripteur matérialisé (YAML composé avec le code + tags) |

## Champs avec règles

```sh
this add entity customer --fields "email:String:required:email:trim,age:Option<i32>:min_value(18):max_value(150),code:String:filter(eq,starts_with):sortable"
```

Règles : `required optional positive string_length(a,b) min_value(n) max_value(n) in_list(a|b)
date_format(f) pattern(re) email url trim uppercase lowercase round_decimals(n) filter(ops) sortable
read_only immutable`, suffixe facultatif `@create` / `@update` (`required` vaut `@create` par défaut).

Les règles de la grammaire de `impl_data_entity_validated!` vont dans la macro ; les autres (email, url,
pattern, min_value, bornes entières, filtres de requête, sortable…) vont dans `DESCRIBED_EXTRA` et la fonction
`described()` du modèle. Ces dernières exigent le mode descripteur (sinon refus : rien ne les appliquerait).

## Commandes

- `this descriptor check [--json] [--strict]` : code de sortie 1 en cas d'erreur (champs redéclarés, politique
  inconnue, tag réutilisé ou déplacé, `check()` du descripteur) ; `--strict` refuse aussi les avertissements
  (lock ou JSON en retard).
- `this descriptor export [--format json|yaml|graphql|proto|json-schema] [--output F|--stdout]`.
- `this descriptor lock` : nouveaux champs → nouveaux tags (≥ 16), champs supprimés → `reserved`.
- `this add module <manifeste>` : exige un autre module depuis son descripteur (copié dans
  `descriptor/modules/<nom>.json`, ajouté à `requires` avec `^<version>`).
- `this info --json` : inclut le descripteur matérialisé.
- `this doctor` : dérive de pluriel, politiques inconnues de links.yaml, dérive de champs, liens vers des
  entités déclarées nulle part, identité du module.

Outils MCP : `descriptor` (init/check/export/lock), `add_module`.
