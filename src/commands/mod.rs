pub mod add_entity;
pub mod add_link;
pub mod add_target;
pub mod build;
pub mod completions;
pub mod dev;
pub mod doctor;
pub mod generate;
pub mod info;
pub mod init;

use clap::{Parser, Subcommand};
use clap_complete::Shell;

/// this - CLI scaffolding tool for this-rs projects
#[derive(Parser)]
#[command(name = "this", version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Simulate operations without writing any files
    #[arg(long, global = true)]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new this-rs project
    Init(InitArgs),

    /// Add components to an existing this-rs project
    Add(AddCommand),

    /// Show project information and status
    Info,

    /// Build the project (API, frontend, or embedded single binary)
    Build(BuildArgs),

    /// Start development servers (API + frontend in parallel)
    Dev(DevArgs),

    /// Generate code from project introspection (TypeScript API client, etc.)
    Generate(GenerateCommand),

    /// Check project health and consistency
    Doctor,

    /// Generate shell completions
    ///
    /// Example: this completions bash > ~/.local/share/bash-completion/completions/this
    #[command(hide = true)]
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },

    /// Start MCP server on stdio for AI agent integration
    #[command(hide = true)]
    Mcp,
}

#[derive(Parser)]
pub struct AddCommand {
    #[command(subcommand)]
    pub command: AddCommands,
}

#[derive(Subcommand)]
pub enum AddCommands {
    /// Add a new entity to the project
    Entity(AddEntityArgs),

    /// Add a link between two entity types
    Link(AddLinkArgs),

    /// Add a deployment target to the workspace (webapp, desktop, mobile)
    Target(AddTargetArgs),
}

/// Arguments for `this init <name>`
#[derive(Parser)]
pub struct InitArgs {
    /// Name of the project to create
    pub name: String,

    /// Parent directory (default: current directory)
    #[arg(long, default_value = ".")]
    pub path: String,

    /// Do not initialize a git repository
    #[arg(long)]
    pub no_git: bool,

    /// Default server port
    #[arg(long, default_value = "3000")]
    pub port: u16,

    /// Path to this-rs crate for local development (uses path dependency instead of crates.io)
    #[arg(long, hide = true)]
    pub this_path: Option<String>,

    /// Create a workspace layout with this.yaml and api/ subdirectory
    #[arg(long)]
    pub workspace: bool,

    /// Enable WebSocket support (adds websocket feature and WebSocketExposure in main.rs)
    #[arg(long)]
    pub websocket: bool,

    /// Enable gRPC support (adds grpc feature and GrpcExposure in main.rs)
    #[arg(long)]
    pub grpc: bool,
}

/// Arguments for `this add entity <name>`
#[derive(Parser)]
pub struct AddEntityArgs {
    /// Entity name (singular, snake_case, e.g. "product")
    pub name: String,

    /// Entity fields as "field:Type" pairs, comma-separated
    /// Example: --fields "sku:String,price:f64,description:Option<String>"
    #[arg(long)]
    pub fields: Option<String>,

    /// Use impl_data_entity_validated! with basic validators
    #[arg(long)]
    pub validated: bool,

    /// Fields to index, comma-separated (default: "name")
    #[arg(long, default_value = "name")]
    pub indexed: String,

    /// Storage backend for the entity store
    /// - in-memory: uses InMemoryDataService (default, no external deps)
    /// - postgres: uses PostgresDataService (requires --features postgres + PgPool)
    #[arg(long, default_value = "in-memory")]
    pub backend: String,
}

/// Arguments for `this add link <source> <target>`
#[derive(Parser)]
pub struct AddLinkArgs {
    /// Source entity type (e.g. "order")
    pub source: String,

    /// Target entity type (e.g. "invoice")
    pub target: String,

    /// Custom link type (default: has_<target>)
    #[arg(long, rename_all = "kebab-case")]
    pub link_type: Option<String>,

    /// Forward route name (default: pluralized target)
    #[arg(long)]
    pub forward: Option<String>,

    /// Reverse route name (default: source)
    #[arg(long)]
    pub reverse: Option<String>,

    /// Link description
    #[arg(long)]
    pub description: Option<String>,

    /// Do not add a validation rule
    #[arg(long)]
    pub no_validation_rule: bool,
}

/// Arguments for `this add target <type>`
#[derive(Parser)]
pub struct AddTargetArgs {
    /// Target type to add
    #[arg(value_enum)]
    pub target_type: crate::config::TargetType,

    /// Frontend framework (for webapp targets)
    #[arg(long, default_value = "react")]
    pub framework: String,

    /// Custom name for the target directory
    #[arg(long)]
    pub name: Option<String>,
}

/// Arguments for `this build`
#[derive(Parser)]
pub struct BuildArgs {
    /// Build a single binary with frontend embedded
    /// (npm build → copy dist → cargo build --features embedded-frontend)
    #[arg(long)]
    pub embed: bool,

    /// Only build the API (cargo build)
    #[arg(long)]
    pub api_only: bool,

    /// Only build the frontend (npm run build)
    #[arg(long)]
    pub front_only: bool,

    /// Generate an optimized multi-stage Dockerfile
    #[arg(long)]
    pub docker: bool,

    /// Build in release mode
    #[arg(long, default_value_t = true)]
    pub release: bool,

    /// Build a specific native target (desktop, ios, android, or "all")
    #[arg(long)]
    pub target: Option<String>,
}

/// Arguments for `this dev`
#[derive(Parser)]
pub struct DevArgs {
    /// Only start the API server (skip frontend dev server)
    #[arg(long)]
    pub api_only: bool,

    /// Disable auto-detection of cargo-watch, force plain cargo run
    #[arg(long)]
    pub no_watch: bool,

    /// Override the API port from this.yaml
    #[arg(long)]
    pub port: Option<u16>,
}

#[derive(Parser)]
pub struct GenerateCommand {
    #[command(subcommand)]
    pub command: GenerateCommands,
}

#[derive(Subcommand)]
pub enum GenerateCommands {
    /// Generate a typed API client from project entities
    Client(GenerateClientArgs),
}

/// Arguments for `this generate client`
#[derive(Parser)]
pub struct GenerateClientArgs {
    /// Target language for the generated client
    #[arg(long, default_value = "typescript")]
    pub lang: String,

    /// Output file path (default: auto-detected from this.yaml webapp target)
    #[arg(long)]
    pub output: Option<std::path::PathBuf>,
}
