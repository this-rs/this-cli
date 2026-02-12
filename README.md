# this-cli

[![CI](https://github.com/this-rs/this-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/this-rs/this-cli/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/this-rs/this-cli/graph/badge.svg)](https://codecov.io/gh/this-rs/this-cli)
[![Crates.io](https://img.shields.io/crates/v/this-cli.svg)](https://crates.io/crates/this-cli)
[![License](https://img.shields.io/crates/l/this-cli.svg)](LICENSE-MIT)

A CLI scaffolding tool for [this-rs](https://github.com/triviere/this-rs) projects.

Generate fully compilable this-rs projects and entities from the command line -- no manual wiring required.

## Features

- **Zero-touch scaffolding** -- `this init` + `this add entity` produces code that compiles and runs immediately
- **WebSocket support** -- `this init --websocket` enables real-time communication via this-rs WebSocket feature
- **gRPC support** -- `this init --grpc` enables Protocol Buffers via this-rs gRPC feature (EntityService + LinkService + proto export at `/grpc/proto`)
- **Workspace mode** -- `this init --workspace` creates a multi-target project with `this.yaml` and `api/` subdirectory
- **Frontend targets** -- `this add target webapp` scaffolds a React/Vue/Svelte SPA with Vite, TypeScript, and API proxy
- **Native targets** -- Desktop (Tauri 2), iOS & Android (Capacitor 6) with `this add target desktop|ios|android`
- **Typed API client generation** -- `this generate client` introspects entities and links to produce a TypeScript API client
- **Embed frontend** -- `this build --embed` produces a single binary with the frontend bundled via rust-embed
- **Dev server orchestration** -- `this dev` runs API + frontend in parallel with auto-reload and colored output
- **Docker support** -- `this build --docker` generates a multi-stage Dockerfile
- **Automatic code registration** -- entities are registered in `module.rs`, `stores.rs`, and `links.yaml` automatically via marker-based insertion
- **Project introspection** -- `this info` shows entities, links, workspace context, and coherence status at a glance
- **Health diagnostics** -- `this doctor` checks project and workspace consistency and reports issues
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

### Classic project

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

### Workspace project (multi-target)

```sh
# Create a workspace with this.yaml + api/ subdirectory
this init my-app --workspace

# All commands work from the workspace root
cd my-app
this add entity product --fields "sku:String,price:f64"

# Add a React frontend
this add target webapp

# Generate a typed TypeScript API client
this generate client

# Start development (API + frontend in parallel)
this dev

# Build a single binary with embedded frontend
this build --embed

# Generate a production Dockerfile
this build --docker
```

The generated project includes:
- A working HTTP server on `http://127.0.0.1:3000`
- Full CRUD endpoints for each entity
- Link routes between related entities
- In-memory stores (ready to swap for persistent backends)

## Commands

| Command | Description |
|---------|-------------|
| `this init <name>` | Create a new this-rs project (classic flat layout) |
| `this init <name> --workspace` | Create a workspace with `this.yaml` and `api/` subdirectory |
| `this add entity <name>` | Add an entity with model, store, handlers, descriptor |
| `this add link <source> <target>` | Configure a relationship between two entities |
| `this add target <type>` | Add a deployment target (webapp, desktop, ios, android) |
| `this generate client` | Generate a typed TypeScript API client from project introspection |
| `this build` | Build the project (API + frontend if configured) |
| `this dev` | Start development servers (API + frontend in parallel) |
| `this info` | Display project summary and coherence status |
| `this doctor` | Run diagnostic checks on project health |
| `this completions <shell>` | Generate shell completion scripts |

All write commands support the `--dry-run` flag to preview changes without writing files.

### this init

```sh
this init my-api                    # Create project in ./my-api
this init my-api --port 8080        # Custom server port
this init my-api --no-git           # Skip git init
this init my-api --websocket        # Enable WebSocket support
this init my-api --grpc             # Enable gRPC support
this init my-api --grpc --websocket # Enable both protocols
this init my-app --workspace        # Create workspace layout (this.yaml + api/)
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

### this add target

```sh
this add target webapp              # Add a React SPA (default framework)
this add target webapp --framework vue    # Vue instead of React
this add target webapp --name dashboard   # Custom directory name
this add target desktop             # Add a Tauri 2 desktop target
this add target ios                 # Add a Capacitor iOS target
this add target android             # Add a Capacitor Android target
```

### this generate client

```sh
this generate client                # Auto-detect output from this.yaml webapp target
this generate client --output ./client.ts  # Custom output path
```

Generates a self-contained TypeScript file with interfaces and CRUD functions for all entities and links. Type mapping: `String` -> `string`, `f64`/`f32`/`i32`/`i64`/`u32`/`u64` -> `number`, `bool` -> `boolean`, `Option<T>` -> `T | null`, `Vec<T>` -> `T[]`.

### this build

```sh
this build                          # Build API + frontend (if configured)
this build --embed                  # Single binary with embedded frontend (rust-embed)
this build --docker                 # Generate a multi-stage Dockerfile
this build --api-only               # Build API only
this build --front-only             # Build frontend only
this build --target desktop         # Build desktop app (cargo tauri build)
this build --target ios             # Build iOS target (npx cap sync ios)
this build --target android         # Build Android target (npx cap sync android)
this build --target all             # Build all configured native targets
```

### this dev

```sh
this dev                            # Start API + frontend in parallel
this dev --api-only                 # API only (skip frontend)
this dev --no-watch                 # Run without file watcher (plain cargo run)
this dev --port 8080                # Custom API port
```

Auto-detects `cargo-watch`, `watchexec`, or `bacon` for live reload. Output is prefixed with colored `[API]`/`[FRONT]` labels. Press `Ctrl+C` to stop all servers.

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
- Workspace mode (`init --workspace`) with `this.yaml` and multi-target layout
- Entity generation (`add entity`) with zero-touch pipeline
- Link configuration (`add link`) with smart defaults
- Automatic `module.rs` / `stores.rs` / `links.yaml` updates
- Build system (`build`) with 5 modes: default, embed, api-only, front-only, docker
- Native target builds (`build --target desktop|ios|android|all`)
- Embedded frontend (`build --embed`) -- single binary with rust-embed + SPA fallback
- Dev server orchestration (`dev`) -- parallel API + frontend with colored output and Ctrl+C handling
- Dockerfile generation (`build --docker`) -- multi-stage Node + Rust + Alpine
- Project introspection (`info`) and diagnostics (`doctor`) with workspace awareness
- Frontend target scaffolding (`add target webapp`) -- React, Vue, or Svelte SPA with Vite + TypeScript
- Native target scaffolding -- Desktop (Tauri 2), iOS & Android (Capacitor 6)
- Typed API client generation (`generate client`) -- TypeScript interfaces and CRUD functions from introspection
- MCP server (`this mcp`) for AI agent integration (9 tools)
- Shell completions, dry-run mode
- 267 tests (175 unit + 72 integration + 20 MCP), CI with fmt/clippy/cross-platform

### Not yet implemented

- PostgreSQL store generation (waiting on this-rs `postgres` feature)
- `this remove entity` / `this remove link`
- OpenAPI generation
- Custom user templates

## Documentation

- [Command Reference](docs/commands.md) -- detailed usage for every command
- [Architecture](docs/architecture.md) -- internal design, templates, markers
- [Contributing](docs/contributing.md) -- build, test, add features

## License

This project is part of the [this-rs](https://github.com/triviere/this-rs) workspace.
