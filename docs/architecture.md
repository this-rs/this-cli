# Architecture

Internal design documentation for this-cli contributors and maintainers.

## Table of Contents

- [Project Structure](#project-structure)
- [Command Dispatch](#command-dispatch)
- [Template Engine](#template-engine)
- [Templates Reference](#templates-reference)
- [Marker System](#marker-system)
- [FileWriter Abstraction](#filewriter-abstraction)
- [Project Detection](#project-detection)
- [Code Generation Flows](#code-generation-flows)

---

## Project Structure

```
src/
├── main.rs                          # Entry point, CLI parsing, writer setup
├── commands/
│   ├── mod.rs                       # Cli struct, Commands enum (clap derive)
│   ├── init.rs                      # `this init` implementation
│   ├── add_entity.rs                # `this add entity` + auto-registration
│   ├── add_link.rs                  # `this add link` + YAML manipulation
│   ├── info.rs                      # `this info` — project introspection
│   ├── doctor.rs                    # `this doctor` — health diagnostics
│   └── completions.rs               # `this completions` — shell autocompletion
├── templates/
│   ├── mod.rs                       # TemplateEngine + custom Tera filters
│   ├── project/                     # Templates for `this init`
│   │   ├── Cargo.toml.tera
│   │   ├── main.rs.tera
│   │   ├── module.rs.tera           # Contains [this:xxx] markers
│   │   ├── stores.rs.tera           # Contains [this:xxx] markers
│   │   ├── entities_mod.rs.tera
│   │   └── links.yaml.tera
│   └── entity/                      # Templates for `this add entity`
│       ├── model.rs.tera
│       ├── model_validated.rs.tera
│       ├── store.rs.tera
│       ├── handlers.rs.tera
│       ├── descriptor.rs.tera
│       └── mod.rs.tera
├── utils/
│   ├── mod.rs
│   ├── file_writer.rs               # FileWriter trait (real + dry-run)
│   ├── markers.rs                   # Marker-based file manipulation
│   ├── naming.rs                    # snake_case, PascalCase, pluralize
│   ├── output.rs                    # Colored terminal output helpers
│   └── project.rs                   # Project root detection
└── tests/
    └── integration.rs               # 38 integration tests + 1 e2e
```

## Command Dispatch

The CLI uses [clap 4](https://docs.rs/clap/4) with derive macros for argument parsing.

### Flow

```
main()
  ├── Cli::parse()                    # clap parses args
  ├── if cli.dry_run → DryRunWriter   # Choose writer implementation
  │   else → RealWriter
  └── run_command(cli, &writer)       # Dispatch to command module
        └── match cli.command
              ├── Init(args)      → commands::init::run(args, writer)
              ├── Add(add)
              │     ├── Entity(args) → commands::add_entity::run(args, writer)
              │     └── Link(args)   → commands::add_link::run(args, writer)
              ├── Info            → commands::info::run()
              ├── Doctor          → commands::doctor::run()
              └── Completions { shell } → commands::completions::run(shell)
```

### Key types (in `commands/mod.rs`)

- `Cli` — top-level struct with `--dry-run` flag and `Commands` subcommand
- `Commands` — enum: `Init`, `Add`, `Info`, `Doctor`, `Completions`
- `AddCommands` — nested enum: `Entity`, `Link`
- `InitArgs`, `AddEntityArgs`, `AddLinkArgs` — argument structs

### Writer injection

Commands that write files (`init`, `add entity`, `add link`) accept `&dyn FileWriter` as a parameter. Commands that only read (`info`, `doctor`, `completions`) don't need it.

---

## Template Engine

Templates are embedded into the binary at compile time via `include_str!` and rendered through [Tera](https://docs.rs/tera/1).

### How it works

1. Each `.tera` file is loaded as a `const &str` via `include_str!`
2. `TemplateEngine::new()` registers all templates in a `Tera` instance
3. Custom Tera filters are registered for naming transformations
4. Templates are rendered with `engine.render(name, &context)`

### Custom Filters

| Filter | Function | Example |
|--------|----------|---------|
| `snake_case` | `naming::to_snake_case()` | `OrderItem` -> `order_item` |
| `pascal_case` | `naming::to_pascal_case()` | `order_item` -> `OrderItem` |
| `pluralize` | `naming::pluralize()` | `category` -> `categories` |

### Template Context Variables

#### Project templates (`this init`)

| Variable | Type | Example |
|----------|------|---------|
| `project_name` | String | `my-api` |
| `project_name_snake` | String | `my_api` |
| `port` | u16 | `3000` |

#### Entity templates (`this add entity`)

| Variable | Type | Example |
|----------|------|---------|
| `entity_name` | String | `product` |
| `entity_pascal` | String | `Product` |
| `entity_plural` | String | `products` |
| `fields` | Vec\<Field\> | `[{name: "sku", rust_type: "String", is_optional: false}]` |
| `indexed_fields` | Vec\<String\> | `["name"]` |
| `validated` | bool | `false` |

---

## Templates Reference

### Project Templates (6)

| Template | Output | Purpose |
|----------|--------|---------|
| `Cargo.toml.tera` | `Cargo.toml` | Project manifest with this-rs dependency, tokio, serde |
| `main.rs.tera` | `src/main.rs` | Server entry point with `ServerBuilder`, stores, module |
| `module.rs.tera` | `src/module.rs` | `Module` trait impl with marker comments for auto-registration |
| `stores.rs.tera` | `src/stores.rs` | Centralized `{Project}Stores` struct with marker comments |
| `entities_mod.rs.tera` | `src/entities/mod.rs` | Empty entity re-exports |
| `links.yaml.tera` | `config/links.yaml` | Empty link configuration structure |

### Entity Templates (6)

| Template | Output | Purpose |
|----------|--------|---------|
| `model.rs.tera` | `model.rs` | `impl_data_entity!` with custom fields |
| `model_validated.rs.tera` | `model.rs` | `impl_data_entity_validated!` with validators |
| `store.rs.tera` | `store.rs` | `{Entity}Store` trait + `InMemory{Entity}Store` |
| `handlers.rs.tera` | `handlers.rs` | 5 Axum handlers (list, get, create, update, delete) |
| `descriptor.rs.tera` | `descriptor.rs` | `EntityDescriptor` with route registration |
| `mod.rs.tera` | `mod.rs` | Public re-exports for all entity types |

---

## Marker System

Markers are specially-formatted comments embedded in generated source files. They serve as insertion points for the `this add entity` command to register new entities without parsing the Rust AST.

### Principle

```rust
fn entity_types(&self) -> Vec<&str> {
    vec![
        // [this:entity_types]       <-- marker
        "product",                    <-- inserted by `this add entity product`
        "category",                   <-- inserted by `this add entity category`
    ]
}
```

### All Markers

#### In `module.rs` (4 markers)

| Marker | Purpose | Inserted content |
|--------|---------|-----------------|
| `[this:entity_types]` | Entity type strings in `entity_types()` | `"product",` |
| `[this:register_entities]` | Descriptor registration in `register_entities()` | `registry.register(Box::new(ProductDescriptor::new_with_creator(...)));` |
| `[this:entity_fetcher]` | Match arm in `get_entity_fetcher()` | `"product" => Some(self.stores.products_entity.clone()),` |
| `[this:entity_creator]` | Match arm in `get_entity_creator()` | `"product" => Some(self.stores.products_entity.clone()),` |

#### In `stores.rs` (3 markers)

| Marker | Purpose | Inserted content |
|--------|---------|-----------------|
| `[this:store_fields]` | Struct fields in `{Project}Stores` | `pub products_store: Arc<dyn ProductStore>,` |
| `[this:store_init_vars]` | Variable initialization in `new_in_memory()` | `let products = Arc::new(InMemoryProductStore::default());` |
| `[this:store_init_fields]` | Struct init fields in `new_in_memory()` | `products_store: products.clone(),` |

### Utility Functions (`utils/markers.rs`)

| Function | Purpose |
|----------|---------|
| `insert_after_marker(content, marker, line)` | Insert a line after a marker, preserving indentation |
| `has_line_after_marker(content, marker, needle)` | Check if content already exists (idempotence) |
| `add_import(content, import_line)` | Add a `use` statement after the last existing import |

### Idempotence

Before inserting, the system checks via `has_line_after_marker()` whether the entity is already registered. This makes `this add entity` safe to run multiple times on the same entity.

### Backward Compatibility

If markers are not found (pre-v0.0.2 projects), a warning is displayed and the auto-registration step is skipped. Manual instructions are shown instead.

---

## FileWriter Abstraction

The `FileWriter` trait abstracts all filesystem writes, enabling the `--dry-run` mode.

### Trait Definition

```rust
pub trait FileWriter {
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn write_file(&self, path: &Path, content: &str) -> Result<()>;
    fn update_file(&self, path: &Path, original: &str, updated: &str) -> Result<()>;
    fn is_dry_run(&self) -> bool;
}
```

### Implementations

| Implementation | Behavior |
|---------------|----------|
| `RealWriter` | Delegates to `std::fs::create_dir_all` / `std::fs::write` |
| `DryRunWriter` | Prints "Would create/modify" messages, tracks operations in `RefCell<Vec<PathBuf>>`, shows simplified diff for updates |

### Interior Mutability

`DryRunWriter` uses `RefCell<Vec<PathBuf>>` for its operation trackers because the `FileWriter` trait takes `&self` (not `&mut self`). This allows it to accumulate state through a shared reference, which is necessary since `writer` is passed as `&dyn FileWriter`.

### Writer Selection (in `main.rs`)

```rust
if dry_run {
    let writer = DryRunWriter::new();
    let res = run_command(cli, &writer);
    writer.print_summary();   // Show "N file(s) would be created"
    res
} else {
    let writer = RealWriter;
    run_command(cli, &writer)
}
```

---

## Project Detection

The function `detect_project_root()` in `utils/project.rs` identifies this-rs projects.

### Algorithm

1. Start from the current working directory
2. Check if `Cargo.toml` exists in the current directory
3. If yes, read it and check if it contains `[dependencies]` and `this` (the this-rs crate)
4. If found, return this directory as the project root
5. If not, move to the parent directory and repeat
6. If the filesystem root is reached without finding a match, return an error

### Used by

- `this add entity` -- to find where to generate entity files
- `this add link` -- to find `config/links.yaml`
- `this info` -- to scan project state
- `this doctor` -- to run diagnostic checks

### Not used by

- `this init` -- creates a new project, so there's no existing project to detect

---

## Code Generation Flows

### `this init <name>`

```
this init my-api
│
├── Create directory: my-api/
├── Create directory: my-api/src/
├── Create directory: my-api/src/entities/
├── Create directory: my-api/config/
│
├── Render & write: project/Cargo.toml.tera  → my-api/Cargo.toml
├── Render & write: project/main.rs.tera     → my-api/src/main.rs
├── Render & write: project/module.rs.tera   → my-api/src/module.rs
├── Render & write: project/stores.rs.tera   → my-api/src/stores.rs
├── Render & write: project/entities_mod     → my-api/src/entities/mod.rs
├── Render & write: project/links.yaml.tera  → my-api/config/links.yaml
├── Write:          .gitignore               → my-api/.gitignore
│
└── Run: git init (unless --no-git)
```

### `this add entity <name>`

```
this add entity product --fields "sku:String,price:f64"
│
├── detect_project_root() → find project directory
├── Parse --fields, filter reserved fields (id, name, status, ...)
│
├── CREATE 5 files:
│   ├── Render entity/model.rs.tera       → src/entities/product/model.rs
│   ├── Render entity/store.rs.tera       → src/entities/product/store.rs
│   ├── Render entity/handlers.rs.tera    → src/entities/product/handlers.rs
│   ├── Render entity/descriptor.rs.tera  → src/entities/product/descriptor.rs
│   └── Render entity/mod.rs.tera         → src/entities/product/mod.rs
│
├── UPDATE src/entities/mod.rs:
│   └── Append: pub mod product;
│
├── UPDATE src/stores.rs (via markers):
│   ├── [this:store_fields]     ← add store + entity fields
│   ├── [this:store_init_vars]  ← add Arc::new(InMemoryProductStore)
│   ├── [this:store_init_fields]← add field initialization
│   └── Add imports at top
│
├── UPDATE src/module.rs (via markers):
│   ├── [this:entity_types]     ← add "product"
│   ├── [this:register_entities]← add registry.register(...)
│   ├── [this:entity_fetcher]   ← add match arm
│   ├── [this:entity_creator]   ← add match arm
│   └── Add imports at top
│
└── UPDATE config/links.yaml:
    └── Add entity to entities[] section
```

### `this add link <source> <target>`

```
this add link product category
│
├── detect_project_root() → find project directory
├── Read and parse config/links.yaml
├── Generate defaults: type=has_category, forward=categories, reverse=product
│
├── UPDATE config/links.yaml:
│   ├── Add entity configs (product, category) to entities[] if missing
│   ├── Add LinkDefinition to links[]
│   └── Add validation_rule to validation_rules{}
│
└── Write updated YAML back to config/links.yaml
```
