# Command Reference

Complete reference for all `this` CLI commands.

## Table of Contents

- [Global Options](#global-options)
- [this init](#this-init)
- [this add entity](#this-add-entity)
- [this add link](#this-add-link)
- [this add target](#this-add-target)
- [this generate client](#this-generate-client)
- [this build](#this-build)
- [this dev](#this-dev)
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
| `--websocket` | false | Enable WebSocket support (adds `websocket` feature to this-rs dependency and `WebSocketExposure` in main.rs) |
| `--grpc` | false | Enable gRPC support (adds `grpc` feature to this-rs dependency and `GrpcExposure` in main.rs) |
| `--workspace` | false | Create a workspace layout with `this.yaml` and `api/` subdirectory |
| `--this-path <PATH>` | (none) | Path to a local this-rs checkout (used with `--workspace`): enables the `frontend` feature on the this-rs dependency and generates the vite-rs frontend wiring |

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

# Enable WebSocket support (adds WebSocketExposure + EventBus)
this init my-api --websocket

# Enable gRPC support (adds GrpcExposure + proto export endpoint)
this init my-api --grpc

# Combine gRPC and WebSocket
this init my-api --grpc --websocket

# Combine WebSocket with workspace mode
this init my-app --workspace --websocket

# Combine gRPC + WebSocket + workspace
this init my-app --workspace --grpc --websocket

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

- The generated project targets this-rs v0.0.7
- `module.rs` and `stores.rs` contain marker comments (`// [this:xxx]`) used by `add entity` for automatic code insertion
- The project compiles immediately with `cargo build` (no entities required)
- In workspace mode, `api/dist/.gitkeep` is created as a placeholder for future frontend embedding
- The workspace `.gitignore` includes frontend-related patterns (`node_modules/`, `dist/`, `.next/`, `.nuxt/`)
- `--websocket` adds `features = ["websocket"]` to the this-rs dependency in `Cargo.toml` and generates `main.rs` with `WebSocketExposure` + `EventBus`. The WebSocket endpoint is available at `ws://127.0.0.1:<port>/ws`
- `--grpc` adds `features = ["grpc"]` to the this-rs dependency in `Cargo.toml` and generates `main.rs` with `GrpcExposure`. The gRPC endpoint is available on the same port as REST, and proto definitions are exported at `http://127.0.0.1:<port>/grpc/proto`
- `--grpc` and `--websocket` can be combined: both features are added (`features = ["websocket", "grpc"]`) and both exposures are merged into the same router
- Optional features (`--websocket`, `--grpc`) can be combined freely with `--workspace`
- With `--workspace --this-path <local this>`, the scaffold targets the local this-rs checkout with its `frontend` feature enabled: `main.rs` is generated around `this::server::frontend::attach_from_env(app)` (dev/disk serving) plus an embedded `embedded_frontend::frontend_router()` branch, and `src/embedded_frontend.rs` is generated as a rust-embed store + `frontend_router()` factory. No `tower-http`/`mime_guess` scaffolding is emitted in that case
- Without `--this-path` (published this-rs v0.0.9), workspace scaffolding is unchanged: the legacy `ServeDir` env fallback in `main.rs` and `mime_guess`-based embedded serving

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

## this add target

Add a deployment target to the workspace. Supports `webapp` (React/Vue/Svelte SPA), `desktop` (Tauri 2), `ios` and `android` (Capacitor 6).

### Synopsis

```
this add target [OPTIONS] <TYPE>
```

### Arguments

| Argument | Required | Values | Description |
|----------|----------|--------|-------------|
| `<TYPE>` | Yes | `webapp`, `desktop`, `ios`, `android` | Type of target to add |

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--framework <FW>` | `react` | Frontend framework for webapp target (`react`, `vue`, `svelte`) |
| `--name <NAME>` | auto | Custom name for the target directory (default: `front` for webapp) |

### Generated Files (webapp)

For `this add target webapp` with default settings:

```
front/
├── index.html               # Vite entry point
├── package.json             # Dependencies (React, TypeScript, Vite); scripts: dev, build, build:fast
├── tsconfig.json            # TypeScript configuration
├── vite.config.ts           # Vite config with API proxy to backend port
├── src/
│   ├── main.tsx             # React entry point
│   ├── App.tsx              # Main component with API connectivity check
│   └── App.css              # Default styles
└── public/
    └── (static assets)
```

### Generated Files (desktop)

For `this add target desktop`:

**Prerequisites**: Tauri CLI (`cargo install tauri-cli`), plus platform-specific dependencies:
- macOS: Xcode Command Line Tools
- Windows: Visual Studio Build Tools (MSVC)
- Linux: `webkit2gtk-4.0` and `libappindicator3`

**Requires**: An existing webapp target (the desktop app wraps the frontend SPA).

```
targets/desktop/
└── src-tauri/
    ├── Cargo.toml           # Tauri 2 + tokio + reqwest + API crate dependency
    ├── tauri.conf.json      # Tauri config (window, identifier, devUrl, frontendDist)
    ├── build.rs             # tauri_build::build()
    ├── src/
    │   └── main.rs          # Entry point: tokio::spawn API server + Tauri webview
    ├── icons/               # Placeholder for app icons
    └── capabilities/
        └── default.json     # Default permissions (core:default, shell:allow-open)
```

The desktop app embeds the API server directly via Rust crate dependency. At startup, it spawns the API server with `tokio::spawn`, waits for it to become healthy (health check loop), then launches the Tauri webview pointing to `http://localhost:<port>`.

### Generated Files (ios / android)

For `this add target ios` or `this add target android`:

**Prerequisites**:
- iOS: macOS + Xcode with iOS simulator
- Android: Android Studio + `ANDROID_HOME` environment variable

**Requires**: An existing webapp target (Capacitor wraps the frontend SPA in a native WebView).

```
targets/ios/                 # or targets/android/
├── package.json             # @capacitor/core, @capacitor/cli, @capacitor/<platform>
├── capacitor.config.ts      # App ID, webDir, server URL, plugins
└── .gitignore               # Native platform directories (ios/, android/)
```

Capacitor wraps the frontend SPA in a native WebView. The `package.json` includes scripts for `sync` (push web assets) and `open` (open native IDE).

### Automatically Updated Files

| File | What changes |
|------|-------------|
| `this.yaml` | Adds a target entry with type, runtime, framework (if applicable), and path |

After adding multiple targets, `this.yaml` looks like:

```yaml
name: my-app
api:
  path: api
  port: 3000
targets:
  - target_type: webapp
    framework: react
    path: front
  - target_type: desktop
    runtime: tauri
    path: targets/desktop
  - target_type: ios
    runtime: capacitor
    path: targets/ios
  - target_type: android
    runtime: capacitor
    path: targets/android
```

### Examples

```sh
# Add a React SPA (default)
this add target webapp

# Add a Vue SPA
this add target webapp --framework vue

# Add a Svelte SPA
this add target webapp --framework svelte

# Custom directory name
this add target webapp --name dashboard

# Add a desktop target (Tauri 2)
this add target desktop

# Add iOS and Android targets (Capacitor 6)
this add target ios
this add target android

# Custom desktop directory name
this add target desktop --name tauri-app

# Preview without creating files
this --dry-run add target webapp
```

### Errors

| Error | Cause |
|-------|-------|
| `Not a this-rs workspace` | Command run outside a workspace (requires `this.yaml`) |
| `Target 'front' already exists in this.yaml` | A target with the same path is already configured |
| `Desktop target requires a webapp target` | Desktop/mobile targets need a webapp target first |
| `Unsupported target type: 'xxx'` | Target type not in the supported list |

### Notes

- Must be run from inside a this-rs workspace (not a classic project)
- After adding a webapp target, `this build`, `this build --embed`, `this build --front-only`, `this build --docker`, and `this dev` will automatically use it
- The generated Vite config includes an API proxy to `http://127.0.0.1:<port>` (from `this.yaml`) — used by the legacy `npm run dev` flow; in the vite-rs flow the SPA is served same-origin by the API, so no proxy is needed
- The generated `package.json` includes a `build:fast` script (`vite build`, fast bundling without typecheck) used by the vite-rs dev loop of `this dev`; `npm run build` remains the typechecked production build
- Desktop and mobile targets require a webapp target as a prerequisite -- the native shell wraps the SPA
- Both iOS and Android can coexist in the same workspace

---

## this generate client

Generate a typed TypeScript API client by introspecting the project's entities and links.

### Synopsis

```
this generate client [OPTIONS]
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--lang <LANG>` | `typescript` | Target language (currently only `typescript` is supported) |
| `--output <PATH>` | auto-detected | Output file path |

### Output Path Resolution

1. If `--output` is specified, uses that path
2. If a webapp target exists in `this.yaml`, outputs to `<webapp_path>/src/api-client.ts`
3. Otherwise, outputs to `<workspace_root>/api-client.ts`

### Generated Output

The generated `api-client.ts` contains:

1. **Configuration** -- `API_BASE` constant from `import.meta.env.VITE_API_URL`
2. **Fetch helper** -- generic `fetchJson<T>()` function with error handling
3. **Interfaces** -- for each entity:
   - `{Entity}` -- full type with `id`, `name`, `status`, `created_at`, `updated_at`, custom fields
   - `Create{Entity}` -- input type for creation (excludes auto-generated fields)
   - `Update{Entity}` -- input type for updates (all fields optional)
4. **CRUD functions** -- for each entity:
   - `list{Entities}()` -- GET `/api/{entities}`
   - `get{Entity}(id)` -- GET `/api/{entities}/{id}`
   - `create{Entity}(data)` -- POST `/api/{entities}`
   - `update{Entity}(id, data)` -- PATCH `/api/{entities}/{id}`
   - `delete{Entity}(id)` -- DELETE `/api/{entities}/{id}`
5. **Link functions** -- for each link:
   - `get{Source}{Targets}(sourceId)` -- GET `/api/{sources}/{id}/{targets}`

### Type Mapping (Rust -> TypeScript)

| Rust Type | TypeScript Type |
|-----------|----------------|
| `String` | `string` |
| `f64`, `f32` | `number` |
| `i32`, `i64`, `u32`, `u64` | `number` |
| `bool` | `boolean` |
| `Uuid` | `string` |
| `Option<T>` | `T \| null` |
| `Vec<T>` | `T[]` |
| `HashMap<K, V>` | `Record<K, V>` |
| `DateTime<Utc>` | `string` |
| `Value` (serde_json) | `unknown` |

### Introspection Sources

The generator reads from:

| Source | What it extracts |
|--------|-----------------|
| `src/entities/*/model.rs` | Entity name, fields (from `impl_data_entity!` macro) |
| `src/entities/*/descriptor.rs` | Plural name, REST routes |
| `config/links.yaml` | Link definitions (source, target, forward route) |

### Examples

```sh
# Generate client (auto-detect output path)
this generate client

# Custom output path
this generate client --output ./shared/api-client.ts

# Preview what would be generated
this --dry-run generate client
```

### Errors

| Error | Cause |
|-------|-------|
| `Not inside a this-rs workspace` | No `this.yaml` found in parent directories |
| `No entities found` | No entity directories with `model.rs` files |
| `Unsupported language: 'xxx'` | Language not in the supported list |

### Notes

- Requires a workspace project (not a classic project)
- The generated client uses native `fetch()` -- no external dependencies
- Regenerate the client after adding new entities or links
- The client is framework-agnostic and works with React, Vue, Svelte, or any TypeScript project

---

## this build

Build the project. Supports multiple modes: default (API + frontend), embed (single binary), api-only, front-only, docker (Dockerfile generation), and native target builds.

### Synopsis

```
this build [OPTIONS]
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--embed` | false | Build a single binary with the frontend embedded via rust-embed |
| `--api-only` | false | Build the API only (skip frontend) |
| `--front-only` | false | Build the frontend only (skip API) |
| `--docker` | false | Generate a multi-stage Dockerfile |
| `--release` | true | Build in release mode (applies to API builds) |
| `--target <NAME>` | (none) | Build a specific native target: `desktop`, `ios`, `android`, or `all` |

### Build Modes

#### Default mode (no flags)

Builds the API with `cargo build --release`, then builds the frontend with `npm run build` if a webapp target is configured. Without a webapp target, builds the API only (no error).

#### `--embed` (Embedded frontend)

Produces a single binary with the frontend bundled inside:

1. Builds the frontend with `npm run build`
2. Copies `front/dist/` to `api/dist/`
3. Builds the API with `cargo build --release --features embedded-frontend`

The generated API includes an `embedded_frontend.rs` module that uses [rust-embed](https://crates.io/crates/rust-embed) to serve static files and provides SPA fallback (unknown routes serve `index.html`).

When the API manifest enables this-rs's `frontend` feature (workspaces scaffolded with `this init --workspace --this-path <local this>`), the binary still embeds `dist/` via rust-embed, but the embedded SPA is served with vite-rs rules — ETag/304 caching, MIME detection, SPA fallback, hashed-asset layouts — through `this::server::frontend::embedded(loader)`, instead of `mime_guess`-based serving. The legacy published-this-rs path (no `frontend` feature) is unchanged.

#### `--api-only`

Builds only the API with `cargo build`. Does not require a webapp target.

#### `--front-only`

Builds only the frontend with `npm run build`. Requires a webapp target.

#### `--docker`

Generates a multi-stage `Dockerfile` at the workspace root:

1. **Stage 1 (Node)**: Builds the frontend
2. **Stage 2 (Rust)**: Builds the API with `--features embedded-frontend`
3. **Stage 3 (Alpine)**: Minimal runtime image

#### `--target desktop` (Desktop build)

Builds a native desktop application using Tauri 2:

1. Builds the frontend with `npm run build` (if webapp target exists)
2. Runs `cargo tauri build` in the `targets/desktop/src-tauri/` directory

**Prerequisites**: `cargo install tauri-cli`

**Artifacts produced**:
- macOS: `.dmg` installer + `.app` bundle
- Windows: `.msi` installer + `.exe`
- Linux: `.deb` + `.AppImage`

#### `--target ios` (iOS build)

Syncs the frontend build to the iOS Capacitor project:

1. Builds the frontend with `npm run build` (if webapp target exists)
2. Runs `npx cap sync ios` in `targets/ios/`

After sync, open Xcode to build and deploy: `npx cap open ios` (from the `targets/ios/` directory).

#### `--target android` (Android build)

Syncs the frontend build to the Android Capacitor project:

1. Builds the frontend with `npm run build` (if webapp target exists)
2. Runs `npx cap sync android` in `targets/android/`

After sync, open Android Studio to build and deploy: `npx cap open android` (from the `targets/android/` directory).

**Artifacts produced** (via Android Studio):
- Debug: `.apk`
- Release: `.aab` (Android App Bundle)

#### `--target all` (Build all native targets)

Builds all configured native targets sequentially. The frontend is built once and shared across all targets.

### Webapp Target Requirement

The flags `--embed`, `--front-only`, and `--docker` require a webapp target in `this.yaml`. Without one, the command fails with a clear error message:

```
Error: No webapp target configured. --embed requires a webapp target.
Add one with: this add target webapp
```

### Examples

```sh
# Default: build API + frontend
this build

# Single binary with embedded frontend
this build --embed

# API only
this build --api-only

# Frontend only
this build --front-only

# Generate Dockerfile
this build --docker

# Build desktop app (Tauri 2)
this build --target desktop

# Build for iOS (Capacitor sync)
this build --target ios

# Build for Android (Capacitor sync)
this build --target android

# Build all native targets
this build --target all

# Preview Dockerfile generation without writing
this --dry-run build --docker
```

### Errors

| Error | Cause |
|-------|-------|
| `Not a this-rs workspace` | Command run outside a workspace |
| `No webapp target configured. --embed requires a webapp target` | `--embed`/`--front-only`/`--docker` used without webapp target |
| `No package.json found in <path>` | Webapp target directory not scaffolded |
| `Target 'xxx' not found in this.yaml` | `--target` references an unconfigured target |
| `No native targets configured` | `--target all` used without any native targets |
| `No Cargo.toml found in src-tauri/` | Desktop target not scaffolded (`this add target desktop` needed) |
| `cargo build failed` | Rust compilation error |
| `npm run build failed` | Frontend build error |
| `cargo tauri build failed` | Tauri build error (check prerequisites) |
| `npx cap sync failed` | Capacitor sync error (check prerequisites) |

### Notes

- Must be run from inside a this-rs workspace (not a classic project)
- Binary size is displayed after `--release` and `--embed` builds
- The `--docker` flag generates the Dockerfile but does not build the Docker image. Run `docker build -t <name> .` afterward.
- The `embedded-frontend` Cargo feature is defined in the generated `Cargo.toml` and is only enabled during `--embed` and Docker builds (legacy scaffolding). Workspaces scaffolded with a local this-rs (`--this-path`) enable the `frontend` feature on the this-rs dependency instead; `--embed` binaries still bundle `dist/` via rust-embed and serve it with vite-rs rules
- `--target` is mutually exclusive with `--embed`, `--api-only`, `--front-only`, and `--docker` -- if `--target` is specified, it takes priority

---

## this dev

Start the development environment: the API and the SPA together, with auto-reload. Two frontend flows exist — the vite-rs single-process flow when the API has this-rs's `frontend` feature, and the legacy parallel `npm run dev` (Node/Vite) flow otherwise.

### Synopsis

```
this dev [OPTIONS]
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--api-only` | false | Start the API server only (skip frontend) |
| `--no-watch` | false | Run without file watcher (plain `cargo run`) |
| `--port <PORT>` | from `this.yaml` | Override the API server port |

### How It Works

1. **Workspace detection**: Finds `this.yaml` and loads the workspace configuration
2. **Frontend mode detection**: Reads the API manifest to decide how to run the frontend:
   - this-rs `frontend` feature enabled → **vite-rs flow** (workspaces scaffolded with `this init --workspace --this-path <local this>`)
   - no `frontend` feature → **legacy flow** (published this-rs v0.0.9 workspaces)
3. **Watcher detection**: Checks for available Rust watchers (in order of preference):
   - `cargo-watch` -- `cargo watch -x run -w src/`
   - `watchexec` -- `watchexec -r -e rs -- cargo run`
   - `bacon` -- `bacon run`
   - None -- falls back to plain `cargo run`
4. **vite-rs flow** (API has the `frontend` feature):
   a. Runs the webapp bundler once — `npm run build:fast` if the webapp's `package.json` defines it, otherwise `npm run build`
   b. Spawns the API server with the detected watcher and env: `PORT`, `SERVE_FRONTEND=true`, `FRONTEND_PATH=<webapp>/dist`, `THIS_DEV=1`, `THIS_WATCH=<webapp>/src`, `THIS_REBUILD=<bundler cmd>`, `THIS_REBUILD_CWD=<webapp>`, `THIS_DEV_LOG=/tmp/<name>-frontend-dev.log`
   c. The API serves the API and the SPA on the **same port** (default 3000): vite-rs watches the webapp sources, rebuilds via `THIS_REBUILD`, hot-reloads the browser, and exposes the `/__dev` AI command bridge. No separate frontend process is spawned
5. **Legacy flow** (no `frontend` feature): if not `--api-only` and a webapp target exists, spawns `npm run dev` (Node/Vite) alongside the API, with a warning
6. **Output streaming**: Process output is streamed with colored prefixes:
   - `[API]` in blue for the API server
   - `[FRONT]` in green for the frontend (legacy flow)
7. **Graceful shutdown**: `Ctrl+C` stops everything cleanly

### Startup Banner

With the `frontend` feature (vite-rs flow), the banner prints the single URL plus the dev log path and a curl bridge example:

```
  🚀 Starting development servers...

   API:  http://127.0.0.1:3000  ✓ cargo-watch
   Front: http://127.0.0.1:3000  (vite-rs dev loop — served by the API process)
        dev log: /tmp/my-app-frontend-dev.log
        bridge: curl -s -X POST localhost:3000/__dev/command -H 'content-type: application/json' -d '{"code":"document.title"}'

   Press Ctrl+C to stop
```

Legacy workspaces (no `frontend` feature) keep the two-URL banner:

```
  🚀 Starting development servers...

   API:   http://127.0.0.1:3000  ✓ cargo-watch
   Front: http://localhost:5173  (front)  ⚠ npm run dev fallback

   Press Ctrl+C to stop
```

### Examples

```sh
# Start both API and frontend
this dev

# API only (skip frontend)
this dev --api-only

# Custom port
this dev --port 8080

# Without file watcher (plain cargo run)
this dev --no-watch
```

### Errors

| Error | Cause |
|-------|-------|
| `Not a this-rs workspace` | Command run outside a workspace |
| `Failed to start API server. Is Rust installed?` | Rust toolchain not available |
| `No package.json found in <path>` | Webapp target directory not scaffolded |

### Notes

- Must be run from inside a this-rs workspace (not a classic project)
- Without a webapp target, runs the API only (no error, just an info message)
- Install a watcher for the best experience: `cargo install cargo-watch`
- In the vite-rs flow the SPA shares the API's port (default 3000): `this dev` does not configure a separate frontend port and the SPA is served by the API process itself (vite-rs), not by a separate Node/Vite dev server
- In the legacy flow (no `frontend` feature), the frontend dev server port (typically 5173 for Vite) is configured in the frontend's own config, not by `this dev`
- In the legacy flow, if the frontend process exits unexpectedly, the API keeps running; in the vite-rs flow the frontend runs in-process, so an API exit stops the whole session
- The `/__dev` AI command bridge is active while the API runs with `THIS_DEV=1`; set `THIS_DEV_COMMANDS=false` to disable it
- If the API process exits, the entire dev session stops

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
3. **Feature flags** -- WebSocket and gRPC status (enabled/disabled), detected from Cargo.toml features
4. **Entities** -- list of entities with their custom fields, parsed from `model.rs` files
5. **Links** -- relationships with forward/reverse routes, parsed from `links.yaml`
6. **Status** -- coherence checks:
   - Module registration (entities in `module.rs` vs. entities on disk)
   - Store configuration (stores in `stores.rs` vs. entities on disk)
   - Link validity (link targets reference existing entities)

### Example Output (Classic project)

```
📦 Project: my-api
   Framework: this-rs v0.0.7

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
   Framework: this-rs v0.0.7

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
| **WebSocket** | If `websocket` feature is enabled in Cargo.toml, verifies that `main.rs` contains `WebSocketExposure` |
| **gRPC** | If `grpc` feature is enabled in Cargo.toml, verifies that `main.rs` contains `GrpcExposure` |

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

  ✅ Cargo.toml — this-rs v0.0.7 detected
  ✅ Entities — 2 entities found, all declared in mod.rs
  ✅ Module — All 2 entities registered
  ✅ Stores — All 2 stores configured
  ✅ Links — Valid configuration (1 links)

Summary: 5 passed
```

### Example Output (Issues)

```
🔍 Checking project: my-api

  ✅ Cargo.toml — this-rs v0.0.7 detected
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
