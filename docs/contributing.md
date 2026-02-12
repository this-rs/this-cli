# Contributing

Guide for developers who want to contribute to this-cli.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Getting Started](#getting-started)
- [Running Tests](#running-tests)
- [Adding a New Command](#adding-a-new-command)
- [Adding a New Template](#adding-a-new-template)
- [Modifying Generated Code](#modifying-generated-code)
- [CI Pipeline](#ci-pipeline)
- [Commit Conventions](#commit-conventions)
- [Code Style](#code-style)

---

## Prerequisites

- **Rust** toolchain (edition 2024) -- install via [rustup](https://rustup.rs/)
- **cargo** with `fmt` and `clippy` components:
  ```sh
  rustup component add rustfmt clippy
  ```
- The this-rs workspace checked out (this-cli lives inside the `this-rs` workspace)

## Getting Started

```sh
# Clone the workspace
git clone https://github.com/triviere/this-rs.git
cd this-rs

# Build this-cli
cargo build -p this-cli

# Run it
./target/debug/this --help

# Or via cargo
cargo run -p this-cli -- --help
```

The binary is named `this` (not `this-cli`), configured in `Cargo.toml`:

```toml
[[bin]]
name = "this"
path = "src/main.rs"
```

## Running Tests

### All tests (fast)

```sh
cargo test -p this-cli
```

This runs 139 unit tests, 57 integration tests, and 17 MCP tests (~0.5s total).

### End-to-end compilation test (slow)

```sh
cargo test -p this-cli -- --ignored
```

This test generates a full project with entities and links, then runs `cargo check` on it to verify the generated code actually compiles against this-rs. Takes ~8s due to dependency resolution.

### Linting

```sh
# Format check (must pass with no diff)
cargo fmt -p this-cli -- --check

# Clippy with warnings as errors
cargo clippy -p this-cli -- -D warnings
```

### Run everything (mimics CI)

```sh
cargo fmt -p this-cli -- --check \
  && cargo clippy -p this-cli -- -D warnings \
  && cargo test -p this-cli \
  && cargo test -p this-cli -- --ignored
```

---

## Adding a New Command

To add a new command (e.g., `this remove entity`), follow these steps:

### 1. Create the command file

Create `src/commands/remove_entity.rs`:

```rust
use anyhow::Result;
use crate::utils::file_writer::FileWriter;

pub fn run(args: RemoveEntityArgs, writer: &dyn FileWriter) -> Result<()> {
    // Implementation here
    Ok(())
}
```

If the command only reads (no file writes), omit the `writer` parameter.

### 2. Register in `commands/mod.rs`

Add the module:

```rust
pub mod remove_entity;
```

Add the args struct:

```rust
#[derive(Args)]
pub struct RemoveEntityArgs {
    /// Entity name to remove
    pub name: String,
}
```

Add the variant to the `Commands` or `AddCommands` enum:

```rust
enum AddCommands {
    Entity(AddEntityArgs),
    Link(AddLinkArgs),
    RemoveEntity(RemoveEntityArgs),  // new
}
```

### 3. Dispatch in `main.rs`

Add the match arm in `run_command()`:

```rust
AddCommands::RemoveEntity(args) => commands::remove_entity::run(args, writer),
```

### 4. Add tests

- Unit tests in the command file (`#[cfg(test)] mod tests`)
- Integration tests in `tests/integration.rs` (using the `run_this()` helper)
- MCP integration tests in `tests/mcp_integration.rs`

### 5. Update documentation

- Add the command to `docs/commands.md`
- Update the commands table in `README.md`

---

## Adding a New Template

To add a new template (e.g., a migration file):

### 1. Create the `.tera` file

Create `src/templates/entity/migration.sql.tera`:

```sql
CREATE TABLE {{ entity_plural }} (
    id UUID PRIMARY KEY,
    name VARCHAR NOT NULL,
{% for field in fields %}    {{ field.name }} {{ field.sql_type }} NOT NULL,
{% endfor %}    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);
```

### 2. Register in `templates/mod.rs`

Add the `include_str!`:

```rust
const TPL_ENTITY_MIGRATION_SQL: &str = include_str!("entity/migration.sql.tera");
```

Add to the templates HashMap:

```rust
("entity/migration.sql", TPL_ENTITY_MIGRATION_SQL),
```

### 3. Render in the command

In the command file (e.g., `add_entity.rs`):

```rust
let rendered = engine.render("entity/migration.sql", &context)?;
writer.write_file(&migrations_dir.join(filename), &rendered)?;
```

### 4. Add template tests

In `templates/mod.rs` tests:

```rust
#[test]
fn test_entity_migration() {
    let engine = TemplateEngine::new().unwrap();
    let result = engine.render("entity/migration.sql", &make_entity_context());
    assert!(result.is_ok());
    let content = result.unwrap();
    assert!(content.contains("CREATE TABLE products"));
    assert!(!content.contains("{{"), "No unresolved Tera placeholders");
}
```

### Available Context Variables

See [Architecture > Template Context Variables](architecture.md#template-context-variables) for the full list of variables available in each template type.

---

## Modifying Generated Code

When changing what code this-cli generates:

### 1. Edit the template

Modify the `.tera` file in `src/templates/project/` or `src/templates/entity/`.

### 2. Run the template unit tests

```sh
cargo test -p this-cli -- templates
```

These tests verify that each template renders without errors and contains expected content.

### 3. Run the e2e compilation test

```sh
cargo test -p this-cli -- --ignored test_generated_code_compiles
```

This is the ultimate validation: it generates a full project and runs `cargo check` on it.

### 4. Test manually

```sh
cd /tmp
rm -rf test-project
this init test-project --no-git
cd test-project
this add entity product --fields "sku:String,price:f64"
cargo check
```

### Important

- Template changes can break existing projects. Test thoroughly.
- If adding new markers, update the marker documentation in [Architecture > Marker System](architecture.md#marker-system).
- Generated code must compile against the currently supported this-rs version (v0.0.6).

---

## CI Pipeline

CI runs on every push to `main` and on pull requests. Defined in `.github/workflows/ci.yml`.

### Jobs

| Job | What it checks | Command |
|-----|---------------|---------|
| **test** | All unit + integration tests pass | `cargo test --verbose` |
| **fmt** | Code is properly formatted | `cargo fmt --all -- --check` |
| **clippy** | No linting warnings | `cargo clippy --all-targets -- -D warnings` |
| **cross-platform** | Builds and tests on Linux, macOS, Windows | `cargo build && cargo test` on 3 OS |

### Running CI locally

```sh
# Reproduce the full CI pipeline
cargo fmt -p this-cli -- --check
cargo clippy -p this-cli -- -D warnings
cargo test -p this-cli
```

The e2e compilation test (`--ignored`) is not part of CI to avoid slow builds.

---

## Commit Conventions

Follow the [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>(<scope>): <description>
```

### Types

| Type | When to use |
|------|-------------|
| `feat` | New feature or command |
| `fix` | Bug fix |
| `refactor` | Code restructuring without behavior change |
| `test` | Adding or modifying tests |
| `docs` | Documentation changes |
| `chore` | Build, CI, dependencies |

### Scopes

| Scope | What it covers |
|-------|---------------|
| `init` | `this init` command |
| `workspace` | Workspace mode (`--workspace`, `this.yaml`, config) |
| `add-entity` | `this add entity` command |
| `add-link` | `this add link` command |
| `build` | `this build` command (all modes) |
| `dev` | `this dev` command (dev server orchestration) |
| `embed` | Embedded frontend (rust-embed templates, feature flag) |
| `info` | `this info` command |
| `doctor` | `this doctor` command |
| `target` | `this add target` command (webapp, desktop, mobile targets) |
| `codegen` | Code generation modules (`codegen/introspect.rs`, `codegen/typescript.rs`) |
| `generate` | `this generate client` command |
| `mcp` | MCP server and tools |
| `completions` | Shell completions |
| `dry-run` | Dry-run mode |
| `templates` | Template changes |
| `lint` | Clippy / formatting fixes |

### Examples

```
feat(info): add `this info` command for project introspection
feat(workspace): add --workspace flag to this init
feat(build): add `this build` command with Docker, embed, and split modes
feat(dev): add `this dev` command for parallel API + frontend development
feat(embed): add rust-embed templates for workspace mode
feat(target): add `this add target webapp` with React/Vue/Svelte scaffolding
feat(codegen): add project introspection module
feat(generate): add `this generate client` command
fix(add-entity): filter reserved fields from impl_data_entity! macro
test(mcp): add workspace integration tests for MCP tools
refactor(templates): extract store initialization into helper
docs: add README.md and documentation
```

---

## Code Style

### Rules

- **Zero warnings**: `cargo clippy -- -D warnings` must pass
- **Formatted**: `cargo fmt` must produce no diff
- **No unused code**: no dead code, unused imports, or unused variables
- **Error handling**: use `anyhow::Result` with `.with_context()` for actionable error messages
- **Output**: use `utils/output.rs` helpers (`print_step`, `print_success`, `print_warn`, `print_error`, `print_info`, `print_file_created`) for consistent terminal output

### Naming

- Files: `snake_case.rs`
- Structs/Enums: `PascalCase`
- Functions/Variables: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Template files: `original_filename.tera` (e.g., `model.rs.tera`)

### Testing

- Unit tests: `#[cfg(test)] mod tests` at the bottom of each file
- Integration tests: `tests/integration.rs`, using the `run_this()` helper
- MCP integration tests: `tests/mcp_integration.rs`, using JSON-RPC stdio protocol
- Slow tests: mark with `#[ignore]`, run with `cargo test -- --ignored`
- Test names: `test_<feature>_<scenario>` (e.g., `test_add_entity_reserved_field_name_filtered`)
