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
- [Workspace Configuration](#workspace-configuration)
- [Code Generation Flows](#code-generation-flows)
- [Embedded Frontend (rust-embed)](#embedded-frontend-rust-embed)

---

## Project Structure

```
src/
├── main.rs                          # Entry point, CLI parsing, writer setup
├── config.rs                        # WorkspaceConfig (this.yaml) load/save
├── commands/
│   ├── mod.rs                       # Cli struct, Commands enum (clap derive)
│   ├── init.rs                      # `this init` (classic + workspace modes)
│   ├── add_entity.rs                # `this add entity` + auto-registration
│   ├── add_link.rs                  # `this add link` + YAML manipulation
│   ├── add_target.rs                # `this add target` — scaffold deployment targets (webapp, desktop, ios, android)
│   ├── generate.rs                  # `this generate client` — typed API client generation
│   ├── build.rs                     # `this build` — 6 modes (default, embed, api-only, front-only, docker, --target)
│   ├── dev.rs                       # `this dev` — vite-rs frontend flow (API serves the SPA) or legacy parallel dev
│   ├── info.rs                      # `this info` — project + workspace introspection
│   ├── doctor.rs                    # `this doctor` — health + workspace diagnostics
│   └── completions.rs               # `this completions` — shell autocompletion
├── codegen/                         # Code generation from project introspection
│   ├── mod.rs                       # Module exports
│   ├── introspect.rs                # Parse entities, descriptors, links from source files
│   └── typescript.rs                # TypeScript API client generator
├── mcp/                             # MCP server (JSON-RPC 2.0 over stdio)
│   ├── mod.rs                       # Module exports
│   ├── protocol.rs                  # MCP protocol types
│   ├── server.rs                    # stdio JSON-RPC server loop
│   ├── tools.rs                     # Tool definitions (9 tools)
│   └── handlers.rs                  # Tool execution handlers
├── templates/
│   ├── mod.rs                       # TemplateEngine + custom Tera filters
│   ├── project/                     # Templates for `this init` (classic) + embed
│   │   ├── Cargo.toml.tera
│   │   ├── main.rs.tera
│   │   ├── module.rs.tera           # Contains [this:xxx] markers
│   │   ├── stores.rs.tera           # Contains [this:xxx] markers
│   │   ├── entities_mod.rs.tera
│   │   ├── links.yaml.tera
│   │   └── embedded_frontend.rs.tera # rust-embed store + SPA fallback; dual-mode (frontend feature)
│   ├── workspace/                   # Templates for `this init --workspace` + build
│   │   ├── this.yaml.tera           # Workspace configuration template
│   │   └── Dockerfile.tera          # Multi-stage Dockerfile (Node + Rust + Alpine)
│   ├── entity/                      # Templates for `this add entity`
│   │   ├── model.rs.tera
│   │   ├── model_validated.rs.tera
│   │   ├── store.rs.tera
│   │   ├── handlers.rs.tera
│   │   ├── descriptor.rs.tera
│   │   └── mod.rs.tera
│   └── targets/
│       ├── webapp/                  # Templates for `this add target webapp`
│       │   ├── package.json.tera
│       │   ├── tsconfig.json.tera
│       │   ├── vite.config.ts.tera
│       │   ├── index.html.tera
│       │   ├── main.tsx.tera
│       │   ├── App.tsx.tera
│       │   └── App.css.tera
│       ├── desktop/                 # Templates for `this add target desktop` (Tauri 2)
│       │   ├── tauri-cargo.toml.tera     # Cargo.toml with Tauri 2, tokio, reqwest
│       │   ├── tauri.conf.json.tera      # Tauri window config, devUrl, frontendDist
│       │   ├── tauri-main.rs.tera        # Entry point: tokio::spawn API + Tauri webview
│       │   ├── tauri-build.rs.tera       # tauri_build::build()
│       │   └── capabilities.json.tera    # Default Tauri permissions
│       └── mobile/                  # Templates for `this add target ios|android` (Capacitor 6)
│           ├── capacitor-package.json.tera  # @capacitor/core + platform deps
│           ├── capacitor.config.ts.tera     # App ID, webDir, server URL
│           └── capacitor-gitignore.tera     # Native platform dirs
├── utils/
│   ├── mod.rs
│   ├── file_writer.rs               # FileWriter trait (real + dry-run + MCP)
│   ├── markers.rs                   # Marker-based file manipulation
│   ├── naming.rs                    # snake_case, PascalCase, pluralize
│   ├── output.rs                    # Colored terminal output helpers
│   └── project.rs                   # Project + workspace root detection
└── tests/
    ├── integration.rs               # 72 integration tests + 1 e2e
    └── mcp_integration.rs           # 20 MCP server integration tests
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
              │     ├── if args.workspace → run_workspace(args, writer)
              │     └── else              → run_classic(args, writer)
              ├── Add(add)
              │     ├── Entity(args) → commands::add_entity::run(args, writer)
              │     ├── Link(args)   → commands::add_link::run(args, writer)
              │     └── Target(args) → commands::add_target::run(args, writer)
              ├── Generate(gen)
              │     └── Client(args) → commands::generate::run(args, writer)
              │           ├── introspect::introspect(api_root) → ProjectIntrospection
              │           ├── typescript::generate(&project)   → String (api-client.ts)
              │           └── writer.write_file(output_path, ts_content)
              ├── Build(args)     → commands::build::run(args, writer)
              │     ├── --target    → run_target_build(name, config, root)
              │     │     ├── desktop → run_build_desktop (cargo tauri build)
              │     │     ├── ios     → run_build_mobile (npx cap sync ios)
              │     │     ├── android → run_build_mobile (npx cap sync android)
              │     │     └── all     → iterate all native targets
              │     ├── --docker    → run_docker(config, webapp, root, writer)
              │     ├── --embed     → run_embed(config, webapp, api_path, root)
              │     ├── --api-only  → run_api_build(api_path, release)
              │     ├── --front-only→ run_front_build(webapp, root)
              │     └── (default)   → run_api_build + run_front_build (if webapp)
              ├── Dev(args)       → commands::dev::run(args)
              │     ├── detect_rust_watcher() → CargoWatch | Watchexec | Bacon | None
              │     ├── detect frontend mode → reads API Cargo.toml for the
              │     │     this-rs `frontend` feature
              │     │     ├── enabled  → vite-rs flow (API serves the SPA in-process)
              │     │     └── disabled → legacy flow (spawn npm run dev)
              │     ├── vite-rs flow (`frontend` feature enabled):
              │     │     ├── run webapp bundler once (npm run build:fast if the
              │     │     │     webapp's package.json defines it, else npm run build)
              │     │     └── spawn API process (with watcher) + env contract:
              │     │           SERVE_FRONTEND=true, FRONTEND_PATH=<webapp>/dist,
              │     │           THIS_DEV=1, THIS_WATCH=<webapp>/src,
              │     │           THIS_REBUILD=<bundler cmd>,
              │     │           THIS_REBUILD_CWD=<webapp>,
              │     │           THIS_DEV_LOG=/tmp/<name>-frontend-dev.log
              │     │         → the API serves API + SPA on one port (vite-rs hot
              │     │           reload + /__dev AI bridge); no frontend process
              │     ├── legacy flow (no `frontend` feature):
              │     │     ├── spawn API process (with watcher)
              │     │     └── spawn frontend process (npm run dev, if applicable)
              │     │           → warning printed; Node/Vite on port 5173
              │     └── wait loop + Ctrl+C graceful shutdown
              ├── Info            → commands::info::run()
              ├── Doctor          → commands::doctor::run()
              ├── Mcp             → mcp::server::run_stdio()
              └── Completions { shell } → commands::completions::run(shell)
```

### Key types (in `commands/mod.rs`)

- `Cli` — top-level struct with `--dry-run` flag and `Commands` subcommand
- `Commands` — enum: `Init`, `Add`, `Generate`, `Build`, `Dev`, `Info`, `Doctor`, `Completions`, `Mcp`
- `AddCommands` — nested enum: `Entity`, `Link`, `Target`
- `GenerateCommands` — nested enum: `Client`
- `InitArgs` — includes `--workspace` flag for workspace mode dispatch
- `BuildArgs` — flags: `--embed`, `--api-only`, `--front-only`, `--docker`, `--release`, `--target`
- `DevArgs` — flags: `--api-only`, `--no-watch`, `--port`
- `AddEntityArgs`, `AddLinkArgs`, `AddTargetArgs` — argument structs
- `GenerateClientArgs` — arguments for `this generate client`

### Writer injection

Commands that write files (`init`, `add entity`, `add link`, `build`) accept `&dyn FileWriter` as a parameter. Commands that only read or spawn processes (`info`, `doctor`, `dev`, `completions`) don't need it.

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
| `websocket` | bool | `false` |
| `grpc` | bool | `false` |
| `workspace` | bool | `false` |

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

### Project Templates (7)

| Template | Output | Purpose |
|----------|--------|---------|
| `Cargo.toml.tera` | `Cargo.toml` | Project manifest with this-rs dependency, tokio, serde. Conditional features: `{% if websocket and grpc %}` → `["websocket", "grpc"]`, `{% elif websocket %}` → `["websocket"]`, `{% elif grpc %}` → `["grpc"]`. When scaffolding with `--this-path`, the this-rs dependency points at the local checkout with its `frontend` feature enabled and the generated project drops `tower-http`/`mime_guess` |
| `main.rs.tera` | `src/main.rs` | Server entry point with `ServerBuilder`, stores, module. `{% if websocket or grpc %}` switches to `build_host()` + `Arc<ServerHost>` + individual exposure merges (`WebSocketExposure`, `GrpcExposure`). Dual-mode frontend wiring: with `--this-path` the template renders `this::server::frontend::attach_from_env(app)` (dev/disk) and an embedded `embedded_frontend::frontend_router()` branch; the legacy published-this-rs variant (no `--this-path`) keeps the `ServeDir` env fallback + `attach_frontend()` cfg branches |
| `module.rs.tera` | `src/module.rs` | `Module` trait impl with marker comments for auto-registration |
| `stores.rs.tera` | `src/stores.rs` | Centralized `{Project}Stores` struct with marker comments |
| `entities_mod.rs.tera` | `src/entities/mod.rs` | Empty entity re-exports |
| `links.yaml.tera` | `config/links.yaml` | Empty link configuration structure |
| `embedded_frontend.rs.tera` | `src/embedded_frontend.rs` | Dual-mode rust-embed module: the legacy published-this-rs variant renders rust-embed + mime_guess SPA fallback (behind `embedded-frontend` feature); with `--this-path` it renders a rust-embed store + `frontend_router()` factory, served with vite-rs rules via `this::server::frontend::embedded(loader)` (no `mime_guess`/`tower-http`) |

### Workspace Templates (2)

| Template | Output | Purpose |
|----------|--------|---------|
| `this.yaml.tera` | `this.yaml` | Workspace configuration (name, api path, port, targets) |
| `Dockerfile.tera` | `Dockerfile` | Multi-stage Docker build (Node frontend → Rust builder → Alpine runtime) |

### Webapp Target Templates (7)

| Template | Output | Purpose |
|----------|--------|---------|
| `package.json.tera` | `package.json` | Dependencies for React/Vue/Svelte + Vite + TypeScript; scripts include `dev`, `build` and `build:fast` (fast `vite build` used by the vite-rs dev loop) |
| `tsconfig.json.tera` | `tsconfig.json` | TypeScript compiler configuration |
| `vite.config.ts.tera` | `vite.config.ts` | Vite config with API proxy to backend port |
| `index.html.tera` | `index.html` | HTML entry point for Vite |
| `main.tsx.tera` | `src/main.tsx` | Framework entry point (React/Vue/Svelte) |
| `App.tsx.tera` | `src/App.tsx` | Main component with API connectivity check |
| `App.css.tera` | `src/App.css` | Default application styles |

### Desktop Target Templates (5) — Tauri 2

| Template | Output | Purpose |
|----------|--------|---------|
| `tauri-cargo.toml.tera` | `Cargo.toml` | Tauri 2 manifest with tokio, reqwest, API crate dependency |
| `tauri.conf.json.tera` | `tauri.conf.json` | Window config, devUrl, frontendDist path, app identifier |
| `tauri-main.rs.tera` | `src/main.rs` | Entry point: `tokio::spawn` API server + health check + Tauri webview |
| `tauri-build.rs.tera` | `build.rs` | Simple `tauri_build::build()` call |
| `capabilities.json.tera` | `capabilities/default.json` | Default permissions (core:default, shell:allow-open) |

**Template context variables:**

| Variable | Type | Example |
|----------|------|---------|
| `project_name` | String | `my-app` |
| `project_name_snake` | String | `my_app` |
| `api_port` | u16 | `3000` |
| `front_path` | String | `../../front/dist` |

### Mobile Target Templates (3) — Capacitor 6

| Template | Output | Purpose |
|----------|--------|---------|
| `capacitor-package.json.tera` | `package.json` | @capacitor/core, @capacitor/cli, @capacitor/\<platform\> dependencies |
| `capacitor.config.ts.tera` | `capacitor.config.ts` | App ID, webDir, server URL, CapacitorHttp plugin |
| `capacitor-gitignore.tera` | `.gitignore` | Native platform directories (ios/, android/) |

**Template context variables:**

| Variable | Type | Example |
|----------|------|---------|
| `project_name` | String | `my-app` |
| `api_port` | u16 | `3000` |
| `front_path` | String | `../../front` |
| `platform` | String | `ios` or `android` |

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
5. Check if `this.yaml` exists in the current directory (workspace detection)
6. If yes, parse it and resolve the API directory from `workspace_config.api.path` (typically `api/`)
7. Verify that the API directory contains a valid `Cargo.toml` with a `this` dependency
8. If found, return the API directory as the project root
9. Move to the parent directory and repeat from step 2
10. If the filesystem root is reached without finding a match, return an error

### Workspace Root Detection

The function `find_workspace_root()` (and `find_workspace_root_from(start)`) walks up from the current directory looking for a `this.yaml` file:

- Returns `Some(path)` if a `this.yaml` is found
- Returns `None` if no workspace is detected
- Used by `info` and `doctor` to display workspace context

### Used by

- `this add entity` -- to find where to generate entity files (resolves through workspace if applicable)
- `this add link` -- to find `config/links.yaml`
- `this info` -- to scan project state + detect workspace context
- `this doctor` -- to run diagnostic checks + validate workspace integrity

### Not used by

- `this init` -- creates a new project, so there's no existing project to detect

---

## Workspace Configuration

The `config.rs` module handles `this.yaml` parsing and serialization.

### Data Model

```rust
struct WorkspaceConfig {
    name: String,               // Workspace name
    api: ApiConfig,             // API target configuration
    targets: Vec<TargetConfig>, // Additional targets (future)
}

struct ApiConfig {
    path: String,               // Relative path to API directory (e.g., "api")
    port: u16,                  // Server port (default: 3000)
}

struct TargetConfig {
    target_type: TargetType,    // Target type enum
    framework: Option<String>,  // Framework (e.g., "react", "vue", "svelte")
    runtime: Option<String>,    // Runtime (future use)
    path: String,               // Relative path to target directory
}

enum TargetType {
    Webapp,                     // Frontend web application (React/Vue/Svelte)
    Website,                    // Static website
    Desktop,                    // Desktop application (Tauri)
    Ios,                        // iOS mobile app (Capacitor)
    Android,                    // Android mobile app (Capacitor)
}
```

### Functions

| Function | Purpose |
|----------|---------|
| `load_workspace_config(path)` | Parse `this.yaml` into `WorkspaceConfig` |
| `save_workspace_config(path, config)` | Serialize `WorkspaceConfig` back to YAML |

### Template

The `workspace/this.yaml.tera` template generates the initial workspace config:

```yaml
name: {{ project_name }}
api:
  path: api
  port: {{ port }}
targets: []
```

---

## Code Generation Flows

### `this init <name>` (Classic mode)

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

### `this init <name> --workspace` (Workspace mode)

```
this init my-app --workspace
│
├── Create directory: my-app/
│
├── Render & write: workspace/this.yaml.tera → my-app/this.yaml
│
├── Create directory: my-app/api/
├── Create directory: my-app/api/src/
├── Create directory: my-app/api/src/entities/
├── Create directory: my-app/api/config/
│
├── Render & write: project/Cargo.toml.tera  → my-app/api/Cargo.toml
├── Render & write: project/main.rs.tera     → my-app/api/src/main.rs
├── Render & write: project/module.rs.tera   → my-app/api/src/module.rs
├── Render & write: project/stores.rs.tera   → my-app/api/src/stores.rs
├── Render & write: project/entities_mod     → my-app/api/src/entities/mod.rs
├── Render & write: project/links.yaml.tera  → my-app/api/config/links.yaml
│
├── Create directory: my-app/api/dist/
├── Write:          .gitkeep                 → my-app/api/dist/.gitkeep
│
├── Write:          .gitignore               → my-app/.gitignore (includes frontend patterns)
│
└── Run: git init (unless --no-git)
```

When `--this-path <local this>` is passed, the same project templates render their `frontend`-feature variants: `Cargo.toml` depends on the local this-rs with `features = ["frontend"]`, `main.rs` wires `this::server::frontend::attach_from_env(app)` (dev/disk) plus an embedded `embedded_frontend::frontend_router()` branch, and `embedded_frontend.rs.tera` renders `api/src/embedded_frontend.rs` as a rust-embed store + `frontend_router()` factory (see [Templates Reference](#templates-reference) and [Embedded Frontend](#embedded-frontend-rust-embed)). Without `--this-path` (published this-rs), the legacy `ServeDir` env fallback + mime_guess embedded serving is generated unchanged.

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

### `this add target webapp`

```
this add target webapp --framework react
│
├── find_workspace_root() → find this.yaml
├── load_workspace_config() → WorkspaceConfig
├── Check for duplicate target path
│
├── CREATE 7 files:
│   ├── Render targets/webapp/package.json.tera     → front/package.json
│   ├── Render targets/webapp/tsconfig.json.tera    → front/tsconfig.json
│   ├── Render targets/webapp/vite.config.ts.tera   → front/vite.config.ts
│   ├── Render targets/webapp/index.html.tera       → front/index.html
│   ├── Render targets/webapp/main.tsx.tera         → front/src/main.tsx
│   ├── Render targets/webapp/App.tsx.tera          → front/src/App.tsx
│   └── Render targets/webapp/App.css.tera          → front/src/App.css
│
└── UPDATE this.yaml:
    └── Add TargetConfig { type: Webapp, framework: "react", path: "front" }
```

### `this generate client`

```
this generate client [--output PATH]
│
├── find_workspace_root() → find this.yaml
├── load_workspace_config() → WorkspaceConfig
├── Resolve API root from config.api.path
│
├── INTROSPECT:
│   ├── Scan src/entities/*/model.rs      → parse impl_data_entity! → EntityMeta[]
│   ├── Scan src/entities/*/descriptor.rs → parse routes, plural   → RouteMeta[]
│   └── Parse config/links.yaml           → LinkMeta[]
│   └── Result: ProjectIntrospection { entities, links }
│
├── GENERATE:
│   └── typescript::generate(&project) → api-client.ts content
│
├── RESOLVE output path:
│   ├── --output flag → use as-is
│   ├── webapp target → <webapp.path>/src/api-client.ts
│   └── fallback      → <workspace>/api-client.ts
│
└── writer.write_file(output_path, ts_content)
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

### `this add target desktop`

```
this add target desktop
│
├── find_workspace_root() → find this.yaml
├── load_workspace_config() → WorkspaceConfig
├── Validate: webapp target exists (prerequisite)
├── Check for duplicate desktop target
│
├── CREATE directories:
│     └── targets/desktop/src-tauri/{src/, icons/, capabilities/}
│
├── CREATE 5 files:
│   ├── Render desktop/tauri-cargo.toml.tera     → targets/desktop/src-tauri/Cargo.toml
│   ├── Render desktop/tauri.conf.json.tera      → targets/desktop/src-tauri/tauri.conf.json
│   ├── Render desktop/tauri-main.rs.tera        → targets/desktop/src-tauri/src/main.rs
│   ├── Render desktop/tauri-build.rs.tera       → targets/desktop/src-tauri/build.rs
│   └── Render desktop/capabilities.json.tera    → targets/desktop/src-tauri/capabilities/default.json
│
└── UPDATE this.yaml:
    └── Add TargetConfig { type: Desktop, runtime: "tauri", path: "targets/desktop" }
```

### `this add target ios` / `this add target android`

```
this add target ios|android
│
├── find_workspace_root() → find this.yaml
├── load_workspace_config() → WorkspaceConfig
├── Validate: webapp target exists (prerequisite)
├── Check for duplicate target (ios/android checked separately)
│
├── CREATE directory:
│     └── targets/<platform>/
│
├── CREATE 3 files:
│   ├── Render mobile/capacitor-package.json.tera  → targets/<platform>/package.json
│   ├── Render mobile/capacitor.config.ts.tera     → targets/<platform>/capacitor.config.ts
│   └── Render mobile/capacitor-gitignore.tera     → targets/<platform>/.gitignore
│
└── UPDATE this.yaml:
    └── Add TargetConfig { type: Ios|Android, runtime: "capacitor", path: "targets/<platform>" }
```

### `this build`

```
this build [--embed | --api-only | --front-only | --docker | --target NAME]
│
├── find_workspace_root() → find this.yaml
├── load_workspace_config() → WorkspaceConfig
├── find_webapp_target() → Option<TargetConfig>
│
└── Dispatch based on flags:
      │
      ├── --target <name>:
      │     ├── if "all" → iterate all native targets
      │     │     ├── run_front_build() (once, if webapp exists)
      │     │     └── for each native target → run_single_target_build()
      │     ├── Find target by name in config.targets
      │     ├── Validate: must be Desktop/Ios/Android (not Webapp)
      │     ├── run_front_build() (if webapp exists)
      │     └── run_single_target_build(target)
      │           ├── Desktop → run_build_desktop()
      │           │     └── cargo tauri build (in src-tauri/)
      │           └── Ios|Android → run_build_mobile()
      │                 └── npx cap sync <platform> (in targets/<platform>/)
      │
      ├── --docker:
      │     ├── require_webapp() → bail if no webapp target
      │     ├── TemplateEngine::new()
      │     ├── Render workspace/Dockerfile.tera → Dockerfile
      │     └── writer.write_file(Dockerfile)
      │
      ├── --embed:
      │     ├── require_webapp() → bail if no webapp target
      │     ├── run_front_build() → npm run build
      │     ├── copy_dir_recursive(front/dist → api/dist)
      │     └── cargo build --release --features embedded-frontend
      │
      ├── --api-only:
      │     └── cargo build [--release]
      │
      ├── --front-only:
      │     ├── require_webapp() → bail if no webapp target
      │     └── npm run build
      │
      └── (default):
            ├── cargo build --release
            └── if webapp → npm run build
                else → print info "No webapp target"
```

### `this dev`

```
this dev [--api-only] [--no-watch] [--port PORT]
│
├── find_workspace_root() → find this.yaml
├── load_workspace_config() → WorkspaceConfig
├── Determine port (args.port || config.api.port)
│
├── detect_rust_watcher()
│     ├── Try: cargo-watch --version → CargoWatch
│     ├── Try: watchexec --version   → Watchexec
│     ├── Try: bacon --version       → Bacon
│     └── Fallback                   → None (plain cargo run)
│
├── Detect frontend mode → reads the API Cargo.toml for the this-rs `frontend` feature
│     ├── enabled  → vite-rs flow (API serves the SPA in-process)
│     └── disabled → legacy flow (spawn npm run dev)
│
├── print_banner() → URLs, watcher info, dev log path, Ctrl+C hint
├── Setup Ctrl+C handler (ctrlc crate + AtomicBool)
│
├── vite-rs flow (API has the `frontend` feature):
│     ├── Run webapp bundler once:
│     │     ├── webapp package.json has "build:fast" → npm run build:fast
│     │     └── else                                 → npm run build
│     └── Spawn API process (with watcher):
│     │     ├── CargoWatch → cargo watch -x run -w src/
│     │     ├── Watchexec  → watchexec -r -e rs -- cargo run
│     │     ├── Bacon      → bacon run
│     │     └── None       → cargo run
│     │     ENV:   PORT=<port>, SERVE_FRONTEND=true,
│     │            FRONTEND_PATH=<webapp>/dist, THIS_DEV=1,
│     │            THIS_WATCH=<webapp>/src, THIS_REBUILD=<bundler cmd>,
│     │            THIS_REBUILD_CWD=<webapp>,
│     │            THIS_DEV_LOG=/tmp/<name>-frontend-dev.log
│     │     → the API serves API + SPA on one port: vite-rs watches the
│     │       webapp sources, rebuilds via THIS_REBUILD, hot-reloads the
│     │       browser and exposes the /__dev AI command bridge (no separate
│     │       frontend process is spawned)
│
├── legacy flow (no `frontend` feature; if !api_only && webapp exists):
│     ├── Spawn API process (watcher + PORT env, as above)
│     └── Spawn frontend process:
│     │     └── npm run dev (current_dir = webapp.path)
│     │       + warning → Node/Vite dev server on port 5173
│
├── Stream output threads:
│     ├── API stdout/stderr  → "[API]"   (blue)
│     └── FRONT stdout/stderr→ "[FRONT]" (green, legacy flow only)
│
├── Wait loop:
│     ├── Check Ctrl+C flag (AtomicBool)
│     ├── Check API process (try_wait) → break if exited
│     ├── Check front process (try_wait) → clear if exited (legacy only)
│     └── Sleep 100ms
│
└── Cleanup:
      ├── Kill API process
      ├── Kill frontend process (legacy only)
      └── Join output threads
```

---

## Code Generation

The `codegen` module provides project introspection and code generation capabilities, used by `this generate client`.

### Pipeline

```
Source files                    Introspection              Code Generation
─────────────                   ──────────────             ───────────────
entities/*/model.rs      ──┐
  impl_data_entity!(...)   ├──► introspect()  ──► ProjectIntrospection
entities/*/descriptor.rs ──┤     (regex parsing)     │
  routes, plural name      │                         ├──► typescript::generate()
config/links.yaml        ──┘                         │      → api-client.ts
  link definitions                                   │
                                                     └──► (future: openapi, etc.)
```

### Introspection (`codegen/introspect.rs`)

Parses the project source files to extract metadata without compiling:

| Function | Input | Output |
|----------|-------|--------|
| `introspect(api_root)` | Path to API directory | `ProjectIntrospection` |
| `parse_entity_model_content(content)` | `model.rs` file content | `Option<EntityMeta>` |
| `parse_descriptor_content(content)` | `descriptor.rs` file content | `(plural, Vec<RouteMeta>)` |
| `parse_links_yaml_content(content)` | `links.yaml` content | `Vec<LinkMeta>` |

**Key data structures:**

```rust
struct ProjectIntrospection {
    entities: Vec<EntityMeta>,  // Sorted by name
    links: Vec<LinkMeta>,
}

struct EntityMeta {
    name: String,              // snake_case (e.g., "product")
    pascal_name: String,       // PascalCase (e.g., "Product")
    plural: String,            // Pluralized (e.g., "products")
    fields: Vec<FieldMeta>,    // Custom fields (not built-in)
    indexed_fields: Vec<String>,
    routes: Vec<RouteMeta>,
}

struct LinkMeta {
    source: String,
    target: String,
    link_type: String,
    forward_route: String,
}
```

**Parsing strategy:** Uses regex on the raw source text (no AST parsing). The `impl_data_entity!` macro has a predictable format that can be reliably matched with:

```rust
r#"impl_data_entity(?:_validated)?\!\(\s*(\w+)\s*,\s*"(\w+)"\s*,\s*\[([^\]]*)\]\s*,\s*\{([^}]*)\}"#
```

### TypeScript Generator (`codegen/typescript.rs`)

Generates a self-contained TypeScript API client from `ProjectIntrospection`:

| Function | Purpose |
|----------|---------|
| `generate(project)` | Produces the complete `api-client.ts` content |
| `rust_type_to_ts(type)` | Maps Rust types to TypeScript types |
| `generate_interface(entity)` | Creates `{Entity}`, `Create{Entity}`, `Update{Entity}` interfaces |
| `generate_crud_functions(entity)` | Creates list, get, create, update, delete functions |
| `generate_link_function(link)` | Creates link traversal function |

The generated client uses native `fetch()` with no external dependencies.

---

## Embedded Frontend (rust-embed)

When `this init --workspace` is used, the generated API project includes support for embedding the frontend as static assets into the binary. There are two scaffolding variants:

- **`--this-path <local this>` (this-rs `frontend` feature)** — `embedded_frontend.rs` is a rust-embed store plus a `frontend_router()` factory. The in-binary store is still rust-embed, but the serving rules (ETag/304 caching, MIME detection, SPA fallback, hashed-asset layouts) come from vite-rs via `this::server::frontend::embedded(loader)`. No `tower-http`/`mime_guess` scaffolding is generated.
- **Published this-rs v0.0.9 (no `--this-path`, no `frontend` feature)** — legacy path, unchanged: rust-embed + `mime_guess` embedded serving with a tower-http `ServeDir` env fallback.

vite-rs is a detached crate living outside this repository (`~/projects/vite-rs`) referenced by path (`../../vite-rs`), the same convention as aletheia's miner.

### How It Works (legacy)

The `embedded_frontend.rs.tera` template generates a module that:

1. Uses [rust-embed](https://crates.io/crates/rust-embed) to embed the contents of `dist/` at compile time
2. Uses [mime_guess](https://crates.io/crates/mime_guess) to determine content types
3. Provides an `attach_frontend()` function that adds routes to the Axum router
4. Implements SPA fallback: any request that doesn't match a static file returns `index.html`

### How It Works (frontend feature)

With `--this-path`, the dual-mode templates render the vite-rs variant instead:

1. `embedded_frontend.rs` defines the rust-embed store and a `frontend_router()` factory (no mime_guess)
2. `main.rs` (embedded) mounts `embedded_frontend::frontend_router()`; assets are served with vite-rs rules through `this::server::frontend::embedded(loader)`
3. `main.rs` (dev/disk) calls `this::server::frontend::attach_from_env(app)`; the env contract below decides whether serving is active and whether the vite-rs dev loop runs

### Generated Code Structure

Legacy published-this-rs scaffolding:

```rust
#[cfg(feature = "embedded-frontend")]
mod embedded_frontend {
    use rust_embed::Embed;

    #[derive(Embed)]
    #[folder = "dist/"]
    struct Assets;

    pub fn attach_frontend(router: Router) -> Router {
        router
            .fallback(static_handler)  // Serve static files or SPA fallback
    }
}
```

`frontend`-feature scaffolding (`--this-path`):

```rust
// embedded_frontend.rs — rust-embed store + router factory
#[derive(rust_embed::Embed)]
#[folder = "dist/"]
struct Assets;

// Assets served with vite-rs rules via this::server::frontend::embedded(loader)
pub fn frontend_router() -> Router { /* ... */ }
```

### Feature Flag

Legacy published-this-rs scaffolding defines the `embedded-frontend` feature in the generated `Cargo.toml`:

```toml
[features]
embedded-frontend = ["rust-embed", "mime_guess"]

[dependencies]
rust-embed = { version = "8", optional = true }
mime_guess = { version = "2", optional = true }
```

With `--this-path`, no `embedded-frontend` feature or `mime_guess` dependency is generated: the scaffold enables the `frontend` feature on the local this-rs dependency instead, and rust-embed remains the in-binary store.

### Serving Modes

**Legacy published-this-rs scaffolding** — the generated `main.rs` supports three modes via `#[cfg(feature)]`:

1. **Embedded** (`--features embedded-frontend`): Static files served from the binary
2. **Filesystem** (default, with `dist/` present): Serves from `dist/` directory via tower-http
3. **API-only** (default, no `dist/`): No frontend serving, API routes only

**`frontend`-feature scaffolding (`--this-path`)**:

1. **Dev/disk** (`attach_from_env(app)`): activated at runtime when `SERVE_FRONTEND=true` (or `THIS_SERVE_FRONTEND=true`); serves `FRONTEND_PATH`/`THIS_FRONTEND_PATH` (default `dist`). `THIS_DEV=1` switches on the vite-rs dev loop (watch, rebuild, hot reload, `/__dev` AI bridge)
2. **Embedded**: the embedded branch mounts `embedded_frontend::frontend_router()`; `dist/` embedded via rust-embed is served with vite-rs rules (`this::server::frontend::embedded(loader)`)
3. **API-only**: no serving env set and no embedded branch compiled — API routes only

### Env Contract (frontend feature)

| Env var | Meaning | Default |
|---------|---------|---------|
| `SERVE_FRONTEND` / `THIS_SERVE_FRONTEND` | `"true"` activates dev/disk serving | unset (off) |
| `FRONTEND_PATH` / `THIS_FRONTEND_PATH` | directory served from disk in dev mode | `dist` |
| `THIS_DEV` | `1` enables the vite-rs dev loop (watch/rebuild/hot reload + `/__dev`) | unset |
| `THIS_WATCH` | extra source directories to watch (colon/semicolon-separated list) | — |
| `THIS_REBUILD` | command run to rebuild the frontend on change | `npm run build` |
| `THIS_REBUILD_CWD` | working directory for `THIS_REBUILD` | — |
| `THIS_DEV_LOG` | path of the vite-rs dev log | `$TMPDIR/this-rs-dev.log` |
| `THIS_DEV_COMMANDS` | `false` disables the `/__dev` AI command bridge | enabled |
| `THIS_POLL_MS` | rebuild poll interval (ms) | `500` |

`this dev` drives this contract for vite-rs workspaces by spawning the API with `SERVE_FRONTEND=true`, `FRONTEND_PATH=<webapp>/dist`, `THIS_DEV=1`, `THIS_WATCH=<webapp>/src`, `THIS_REBUILD=<bundler cmd>`, `THIS_REBUILD_CWD=<webapp>` and `THIS_DEV_LOG=/tmp/<name>-frontend-dev.log`; the API then serves the API and the SPA on one port with vite-rs hot reload and the `/__dev` AI command bridge.

---

## Native Target Architecture

### Overview

Native targets extend a this-rs workspace to deploy the same application as a desktop app or mobile app. All native targets wrap the frontend SPA -- they are not standalone; they require a webapp target as a prerequisite.

```
                 ┌──────────────────────────┐
                 │     Frontend SPA          │
                 │  (React / Vue / Svelte)   │
                 └──────────┬───────────────┘
                            │
          ┌─────────────────┼─────────────────┐
          │                 │                  │
    ┌─────▼──────┐   ┌─────▼──────┐   ┌──────▼──────┐
    │   Desktop   │   │    iOS     │   │   Android   │
    │  (Tauri 2)  │   │(Capacitor) │   │(Capacitor)  │
    └─────┬──────┘   └─────┬──────┘   └──────┬──────┘
          │                 │                  │
          │ Rust native     │ Native WebView   │ Native WebView
          │ webview +       │ + HTTP to API    │ + HTTP to API
          │ embedded API    │                  │
          │                 │                  │
    ┌─────▼──────┐   ┌─────▼──────┐   ┌──────▼──────┐
    │ API Server  │   │ API Server │   │ API Server  │
    │ (in-process)│   │ (separate) │   │ (separate)  │
    └────────────┘   └────────────┘   └─────────────┘
```

### Desktop (Tauri 2)

**Architecture**: The API server runs in-process. The Tauri entry point uses `tokio::spawn` to start the Axum API server on a background task, then launches the Tauri webview pointing to `http://localhost:<port>`. A health check loop (300 retries x 100ms = 30s timeout) ensures the API is ready before the window appears.

**Key files**:
- `targets/desktop/src-tauri/Cargo.toml` — depends on the API crate directly (`{{ project_name }} = { path = "../../api" }`)
- `targets/desktop/src-tauri/src/main.rs` — `#[tokio::main]` entry point
- `targets/desktop/src-tauri/tauri.conf.json` — window config, `devUrl: http://localhost:5173`, `frontendDist: ../../front/dist`

**Build**: `cargo tauri build` (via `this build --target desktop`) produces a platform-specific installer.

**Development**: Run `this dev` for the API + frontend, then `cd targets/desktop/src-tauri && cargo tauri dev` for the desktop shell with hot reload.

### Mobile (Capacitor 6)

**Architecture**: Capacitor wraps the frontend SPA in a native WebView shell. Unlike Tauri, the API server runs separately -- the mobile app communicates with the API over HTTP (typically the local network during development, or a deployed URL in production).

**Key files**:
- `targets/ios/package.json` — Capacitor dependencies (`@capacitor/core`, `@capacitor/ios`)
- `targets/ios/capacitor.config.ts` — points `webDir` to `../../front/dist`, `server.url` to `http://localhost:<port>`
- `targets/android/` — same structure, with `@capacitor/android`

**Build**: `this build --target ios|android` runs `npx cap sync <platform>` which copies web assets into the native project. Then open the native IDE (Xcode or Android Studio) to build and deploy.

**Development**: Run `this dev` for the API + frontend. After the first `npx cap sync`, open the native IDE for device/simulator testing.

### Shared Principles

1. **Webapp prerequisite**: All native targets require a webapp target. The SPA is the shared UI layer.
2. **No code rewrite**: Native targets wrap the same SPA -- zero frontend code changes needed.
3. **Independent addition**: Desktop, iOS, and Android can be added independently and coexist.
4. **Single build pipeline**: `this build --target all` builds everything sequentially, sharing the frontend build.
