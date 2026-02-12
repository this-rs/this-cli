# Command Reference

Complete reference for all `this` CLI commands.

## Table of Contents

- [Global Options](#global-options)
- [this init](#this-init)
- [this add entity](#this-add-entity)
- [this add link](#this-add-link)
- [this info](#this-info)
- [this doctor](#this-doctor)
- [this completions](#this-completions)

---

## Global Options

These options are available on all commands:

| Option | Description |
|--------|-------------|
| `--dry-run` | Simulate operations without writing any files |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

### --dry-run

When `--dry-run` is passed, the CLI previews all file operations without actually performing them:

- **New files** are shown as `Would create: <path>`
- **Modified files** show a simple diff with added lines prefixed by `+`
- **Summary** shows the total count of operations that would be performed

```
$ this --dry-run init my-api
🔍 Dry run — no files will be written

  Would create: ./my-api/Cargo.toml
  Would create: ./my-api/src/main.rs
  Would create: ./my-api/src/module.rs
  Would create: ./my-api/src/stores.rs
  Would create: ./my-api/src/entities/mod.rs
  Would create: ./my-api/config/links.yaml
  Would create: .gitignore
  Would run: git init

  6 file(s) would be created
```

---

## this init

Create a new this-rs project with a complete, compilable project structure.

### Synopsis

```
this init [OPTIONS] <NAME>
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<NAME>` | Yes | Name of the project to create |

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--path <PATH>` | `.` | Parent directory for the project |
| `--no-git` | false | Do not initialize a git repository |
| `--port <PORT>` | `3000` | Default server port in `main.rs` |
| `--workspace` | false | Create a workspace layout with `this.yaml` and `api/` subdirectory |

### Generated Files (Classic mode)

```
<name>/
├── Cargo.toml              # Project manifest with this-rs dependency
├── .gitignore              # Rust gitignore
├── src/
│   ├── main.rs             # Server entry point with ServerBuilder
│   ├── module.rs           # Module trait implementation (with markers)
│   ├── stores.rs           # Centralized store struct (with markers)
│   └── entities/
│       └── mod.rs          # Entity re-exports (empty initially)
└── config/
    └── links.yaml          # Link configuration (empty initially)
```

### Generated Files (Workspace mode: `--workspace`)

```
<name>/
├── this.yaml               # Workspace configuration (name, api path, port, targets)
├── .gitignore              # Workspace gitignore (includes node_modules/, dist/, .next/, .nuxt/)
├── api/                    # API target (classic this-rs scaffold)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs
│   │   ├── module.rs
│   │   ├── stores.rs
│   │   └── entities/
│   │       └── mod.rs
│   ├── config/
│   │   └── links.yaml
│   └── dist/
│       └── .gitkeep        # Placeholder for future frontend embed
└── (future targets: webapp/, mobile/, ...)
```

The `this.yaml` file is the workspace source of truth:

```yaml
name: my-app
api:
  path: api
  port: 3000
targets: []
```

When inside a workspace, all commands (`add entity`, `info`, `doctor`) automatically resolve to the `api/` directory via `this.yaml`.

### Examples

```sh
# Basic project creation (classic flat layout)
this init my-api

# Custom port
this init my-api --port 8080

# Skip git initialization
this init my-api --no-git

# Create in a specific directory
this init my-api --path /tmp/projects

# Create a workspace layout for multi-target projects
this init my-app --workspace

# Workspace with custom port
this init my-app --workspace --port 8080

# Preview workspace creation without writing files
this --dry-run init my-app --workspace
```

### Errors

| Error | Cause |
|-------|-------|
| `Directory 'my-api' already exists` | Target directory already exists |

### Notes

- The generated project targets this-rs v0.0.6
- `module.rs` and `stores.rs` contain marker comments (`// [this:xxx]`) used by `add entity` for automatic code insertion
- The project compiles immediately with `cargo build` (no entities required)
- In workspace mode, `api/dist/.gitkeep` is created as a placeholder for future frontend embedding
- The workspace `.gitignore` includes frontend-related patterns (`node_modules/`, `dist/`, `.next/`, `.nuxt/`)

---

## this add entity

Add a new entity to an existing this-rs project. Generates all required files and automatically registers the entity in `module.rs`, `stores.rs`, and `links.yaml`.

### Synopsis

```
this add entity [OPTIONS] <NAME>
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<NAME>` | Yes | Entity name (singular, snake_case, e.g. `product`) |

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--fields <FIELDS>` | (none) | Entity fields as `"field:Type"` pairs, comma-separated |
| `--validated` | false | Use `impl_data_entity_validated!` with validators |
| `--indexed <INDEXED>` | `name` | Fields to index, comma-separated |

### Supported Field Types

| Type | Rust Type | Example |
|------|-----------|---------|
| `String` | `String` | `sku:String` |
| `f64` | `f64` | `price:f64` |
| `f32` | `f32` | `score:f32` |
| `i32` | `i32` | `count:i32` |
| `i64` | `i64` | `timestamp:i64` |
| `u32` | `u32` | `quantity:u32` |
| `u64` | `u64` | `total:u64` |
| `bool` | `bool` | `active:bool` |
| `Uuid` | `Uuid` | `ref_id:Uuid` |
| `Option<T>` | `Option<T>` | `description:Option<String>` |

### Reserved Fields

The following fields are automatically provided by the `impl_data_entity!` macro and will be **filtered out** with a warning if specified in `--fields`:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `Uuid` | Unique identifier |
| `name` | `String` | Entity name |
| `entity_type` | `String` | Type discriminator |
| `status` | `String` | Current status |
| `created_at` | `DateTime<Utc>` | Creation timestamp |
| `updated_at` | `DateTime<Utc>` | Last update timestamp |
| `deleted_at` | `Option<DateTime<Utc>>` | Soft delete timestamp |

### Generated Files

For an entity named `product`:

| File | Description |
|------|-------------|
| `src/entities/product/model.rs` | Entity struct via `impl_data_entity!` |
| `src/entities/product/store.rs` | Store trait + `InMemoryProductStore` |
| `src/entities/product/handlers.rs` | Axum handlers (list, get, create, update, delete) |
| `src/entities/product/descriptor.rs` | `EntityDescriptor` implementation with routes |
| `src/entities/product/mod.rs` | Module re-exports |

### Automatically Updated Files

| File | What changes |
|------|-------------|
| `src/entities/mod.rs` | Adds `pub mod product;` |
| `src/stores.rs` | Adds store fields, imports, and initialization |
| `src/module.rs` | Registers entity in `entity_types()`, `register_entities()`, `get_entity_fetcher()`, `get_entity_creator()` |
| `config/links.yaml` | Adds entity to the `entities` section |

### Zero-Touch Pipeline

After `add entity`, the project compiles and runs without any manual editing:

```sh
this init my-api
cd my-api
this add entity product --fields "sku:String,price:f64"
cargo run   # Starts immediately on :3000
```

### Examples

```sh
# Basic entity
this add entity product --fields "sku:String,price:f64,description:Option<String>"

# Validated entity (with input validation)
this add entity user --fields "email:String,age:i32" --validated

# Custom indexed fields
this add entity article --fields "title:String,body:String" --indexed "title"

# Entity with no extra fields (only built-in fields)
this add entity tag

# Preview what would be generated
this --dry-run add entity product --fields "sku:String"
```

### Errors

| Error | Cause |
|-------|-------|
| `Entity 'product' already exists` | Entity directory already present |
| `Not a this-rs project` | No `Cargo.toml` with this-rs dependency found |
| `Invalid field format: 'xxx'` | Field doesn't match `name:Type` format |
| `Unsupported field type: 'xxx'` | Type not in the supported list |

### Notes

- Entity names are automatically normalized to `snake_case`
- Struct names are converted to `PascalCase` (e.g., `order_item` -> `OrderItem`)
- Pluralization is automatic (e.g., `category` -> `categories`)
- Operations are **idempotent**: adding an already-registered entity skips the registration step
- If `module.rs` or `stores.rs` lack marker comments (e.g., pre-v0.0.2 projects), a warning is shown and manual registration instructions are displayed

---

## this add link

Add a relationship between two entity types in `config/links.yaml`.

### Synopsis

```
this add link [OPTIONS] <SOURCE> <TARGET>
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<SOURCE>` | Yes | Source entity type (e.g. `order`) |
| `<TARGET>` | Yes | Target entity type (e.g. `invoice`) |

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--link-type <TYPE>` | `has_<target>` | Custom link type identifier |
| `--forward <ROUTE>` | pluralized target | Forward route name |
| `--reverse <ROUTE>` | source | Reverse route name |
| `--description <DESC>` | (none) | Link description |
| `--no-validation-rule` | false | Skip adding a validation rule |

### Default Value Generation

The CLI generates sensible defaults automatically:

| Parameter | Rule | Example (product -> category) |
|-----------|------|-------------------------------|
| Link type | `has_<target>` | `has_category` |
| Forward route | `pluralize(target)` | `categories` |
| Reverse route | `source` | `product` |

### Generated REST Routes

For `this add link product category`:

| Method | Route | Description |
|--------|-------|-------------|
| `GET` | `/products/{id}/categories` | List categories for a product |
| `POST` | `/products/{id}/categories` | Link a category to a product |
| `GET` | `/categories/{id}/product` | Get product for a category |

### Examples

```sh
# Basic link with auto-generated defaults
this add link product category

# Custom link type
this add link order invoice --link-type "has_invoice"

# Custom route names
this add link product tag --forward "tags" --reverse "products"

# With description
this add link user role --description "User role assignment"

# Without validation rule
this add link product tag --no-validation-rule
```

### Errors

| Error | Cause |
|-------|-------|
| `Link 'product -> category' (has_category) already exists` | Duplicate link |
| `Not a this-rs project` | No this-rs project detected |
| `Failed to parse links.yaml` | Corrupted YAML file |

### Notes

- Both source and target entities are automatically added to the `entities` section of `links.yaml` if not already present
- Entity auth defaults to `authenticated` for all operations (list, get, create, update, delete)

---

## this info

Display a summary of the current this-rs project: entities, links, workspace context, and coherence status.

### Synopsis

```
this info
```

### Output Sections

1. **Workspace** (if inside a workspace) -- workspace name, API path, port, configured targets
2. **Project** -- name (from `Cargo.toml`) and this-rs version
3. **Entities** -- list of entities with their custom fields, parsed from `model.rs` files
4. **Links** -- relationships with forward/reverse routes, parsed from `links.yaml`
5. **Status** -- coherence checks:
   - Module registration (entities in `module.rs` vs. entities on disk)
   - Store configuration (stores in `stores.rs` vs. entities on disk)
   - Link validity (link targets reference existing entities)

### Example Output (Classic project)

```
📦 Project: my-api
   Framework: this-rs v0.0.6

📋 Entities (2):
   • category (fields: slug)
   • product (fields: sku, price, description)

🔗 Links (1):
   • product → category (has_category)
     ↳ Forward: /products/{id}/categories
     ↳ Reverse: /categories/{id}/product

📊 Status:
   ✅ Module: 2/2 entities registered
   ✅ Stores: 2/2 stores configured
   ✅ Links: Valid configuration
```

### Example Output (Workspace project)

```
🏗️ Workspace: my-app
   API: api (port 3000)
   Targets: (none)

📦 Project: my-app
   Framework: this-rs v0.0.6

📋 Entities (1):
   • product (fields: sku, price)

📊 Status:
   ✅ Module: 1/1 entities registered
   ✅ Stores: 1/1 stores configured
   ✅ Links: Valid configuration
```

### Errors

| Error | Cause |
|-------|-------|
| `Not a this-rs project` | No this-rs project detected in current or parent directories |

### Notes

- Must be run inside a this-rs project directory (or a workspace containing `this.yaml`)
- When run from a workspace root, automatically resolves to the API directory via `this.yaml`
- Displays workspace section (name, API path, port, targets) when inside a workspace
- Works on both pre-v0.0.2 projects (without markers) and v0.0.2+ projects
- Fields are parsed from `impl_data_entity!` blocks in each entity's `model.rs`

---

## this doctor

Run diagnostic checks on project health and consistency.

### Synopsis

```
this doctor
```

### Checks Performed

| Check | What it verifies |
|-------|-----------------|
| **Workspace** (if applicable) | `this.yaml` is parseable, `api/Cargo.toml` exists, target directories are present |
| **Cargo.toml** | this-rs dependency exists and version is detected |
| **Entities** | All entity directories in `src/entities/` are declared in `entities/mod.rs` |
| **Module** | All entities are registered in `module.rs` (via markers) |
| **Stores** | All entities have stores configured in `stores.rs` (via markers) |
| **Links** | All entities referenced in `links.yaml` exist as actual entities |

### Diagnostic Levels

| Level | Symbol | Meaning |
|-------|--------|---------|
| Pass | `✅` | Check passed, no issues |
| Warning | `⚠️` | Non-critical issue (e.g., orphan entity) |
| Error | `❌` | Critical issue that needs fixing |

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | All checks passed (or warnings only) |
| `1` | One or more errors detected |

### Example Output (Healthy)

```
🔍 Checking project: my-api

  ✅ Cargo.toml — this-rs v0.0.6 detected
  ✅ Entities — 2 entities found, all declared in mod.rs
  ✅ Module — All 2 entities registered
  ✅ Stores — All 2 stores configured
  ✅ Links — Valid configuration (1 links)

Summary: 5 passed
```

### Example Output (Issues)

```
🔍 Checking project: my-api

  ✅ Cargo.toml — this-rs v0.0.6 detected
  ⚠️ Entities — Entity 'review' has directory but is not in mod.rs
  ✅ Module — All 1 entities registered
  ✅ Stores — All 1 stores configured
  ❌ Links — Link references unknown entity 'review'

Summary: 3 passed, 1 warning, 1 error
```

### Notes

- Read-only: `doctor` never modifies any files
- When run from a workspace root, automatically resolves to the API directory and also checks workspace integrity
- Workspace checks include: `this.yaml` validity, API directory existence, and target directory presence
- A future `--fix` flag is planned to auto-correct simple issues

---

## this completions

Generate shell completion scripts for autocompletion support.

This is a hidden command (not shown in `--help`) that generates completion scripts via [clap_complete](https://docs.rs/clap_complete).

### Synopsis

```
this completions <SHELL>
```

### Arguments

| Argument | Required | Values |
|----------|----------|--------|
| `<SHELL>` | Yes | `bash`, `zsh`, `fish`, `powershell` |

### Installation

#### Bash

```sh
this completions bash > ~/.local/share/bash-completion/completions/this
# Or system-wide:
this completions bash | sudo tee /etc/bash_completion.d/this > /dev/null
```

#### Zsh

```sh
# Ensure ~/.zfunc is in your fpath (add to .zshrc: fpath=(~/.zfunc $fpath))
this completions zsh > ~/.zfunc/_this
# Then reload:
autoload -Uz compinit && compinit
```

#### Fish

```sh
this completions fish > ~/.config/fish/completions/this.fish
```

#### PowerShell

```powershell
this completions powershell >> $PROFILE.CurrentUserAllHosts
# Then reload your profile
```

### Notes

- Completions include all subcommands, arguments, and options
- Regenerate completions after upgrading `this` to pick up new commands
