# this-cli

A CLI scaffolding tool for [this-rs](https://github.com/triviere/this-rs) projects.

Generate fully compilable this-rs projects and entities from the command line -- no manual wiring required.

## Features

- **Zero-touch scaffolding** -- `this init` + `this add entity` produces code that compiles and runs immediately
- **Automatic code registration** -- entities are registered in `module.rs`, `stores.rs`, and `links.yaml` automatically via marker-based insertion
- **Project introspection** -- `this info` shows entities, links, and coherence status at a glance
- **Health diagnostics** -- `this doctor` checks project consistency and reports issues
- **Dry-run mode** -- preview all file operations before they happen with `--dry-run`
- **Shell completions** -- autocompletion for bash, zsh, fish, and PowerShell
- **Idempotent operations** -- running `add entity` twice won't duplicate registrations

## Installation

### From source (workspace)

```sh
# From the this-rs workspace root
cargo install --path this-cli
```

### Build and run locally

```sh
cargo build -p this-cli
./target/debug/this --help
```

## Quick Start

```sh
# Create a new project
this init my-api

# Add entities
cd my-api
this add entity product --fields "sku:String,price:f64,description:String"
this add entity category --fields "slug:String"

# Link them
this add link product category

# Run -- it compiles and starts immediately
cargo run
```

The generated project includes:
- A working HTTP server on `http://127.0.0.1:3000`
- Full CRUD endpoints for each entity
- Link routes between related entities
- In-memory stores (ready to swap for persistent backends)

## Commands

| Command | Description |
|---------|-------------|
| `this init <name>` | Create a new this-rs project |
| `this add entity <name>` | Add an entity with model, store, handlers, descriptor |
| `this add link <source> <target>` | Configure a relationship between two entities |
| `this info` | Display project summary and coherence status |
| `this doctor` | Run diagnostic checks on project health |
| `this completions <shell>` | Generate shell completion scripts |

All write commands support the `--dry-run` flag to preview changes without writing files.

### this init

```sh
this init my-api                    # Create project in ./my-api
this init my-api --port 8080        # Custom server port
this init my-api --no-git           # Skip git init
this --dry-run init my-api          # Preview without creating files
```

### this add entity

```sh
this add entity product --fields "sku:String,price:f64"
this add entity user --fields "email:String" --validated
this add entity tag --indexed "label"
```

Supported field types: `String`, `f64`, `f32`, `i32`, `i64`, `u32`, `u64`, `bool`, `Uuid`.

Built-in fields (`id`, `name`, `status`, `entity_type`, `created_at`, `updated_at`, `deleted_at`) are provided by the framework and automatically filtered if specified.

### this add link

```sh
this add link product category
this add link order invoice --link-type "has_invoice"
this add link product tag --forward "tags" --reverse "products"
```

Default values are generated automatically:
- Link type: `has_<target>` (e.g., `has_category`)
- Forward route: pluralized target (e.g., `/products/{id}/categories`)
- Reverse route: source (e.g., `/categories/{id}/product`)

### this info

```
$ this info
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

### this doctor

```
$ this doctor
🔍 Checking project: my-api

  ✅ Cargo.toml — this-rs v0.0.6 detected
  ✅ Entities — 2 entities found, all declared in mod.rs
  ✅ Module — All 2 entities registered
  ✅ Stores — All 2 stores configured
  ✅ Links — Valid configuration (1 links)

Summary: 5 passed
```

Exit codes: `0` on success (pass/warnings only), `1` on errors.

## Shell Completions

```sh
# Bash
this completions bash > ~/.local/share/bash-completion/completions/this

# Zsh
this completions zsh > ~/.zfunc/_this

# Fish
this completions fish > ~/.config/fish/completions/this.fish

# PowerShell
this completions powershell > $PROFILE.CurrentUserAllHosts
```

## Project Status

**Version: 0.0.1** (development)

### Implemented

- Project scaffolding (`init`) with compilable output
- Entity generation (`add entity`) with zero-touch pipeline
- Link configuration (`add link`) with smart defaults
- Automatic `module.rs` / `stores.rs` / `links.yaml` updates
- Project introspection (`info`) and diagnostics (`doctor`)
- Shell completions, dry-run mode
- 80 tests (42 unit + 38 integration), CI with fmt/clippy/cross-platform

### Not yet implemented

- PostgreSQL store generation (waiting on this-rs `postgres` feature)
- Multi-module support
- `this remove entity` / `this remove link`
- OpenAPI generation
- Custom user templates
- Hot-reload / watch mode

## Documentation

- [Command Reference](docs/commands.md) -- detailed usage for every command
- [Architecture](docs/architecture.md) -- internal design, templates, markers
- [Contributing](docs/contributing.md) -- build, test, add features

## License

This project is part of the [this-rs](https://github.com/triviere/this-rs) workspace.
