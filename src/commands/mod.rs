pub mod add_entity;
pub mod add_event_flow;
pub mod add_link;
pub mod add_sink;
pub mod add_target;
pub mod adopt_obrain;
pub mod build;
pub mod completions;
pub mod descriptor;
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

    /// Adopt this-rs + obrain + IAM in a project that already exists
    ///
    /// Prepares the ground without deciding anything: enables the `obrain`
    /// feature, inserts the wiring where `this init` left a marker (or shows
    /// the snippet), and prints the adoption order — observe first, then arm.
    /// See docs/adoption-projet-existant.md.
    Adopt(AdoptCommand),

    /// Show project information and status
    Info(InfoArgs),

    /// Module descriptor: init, check (exit code 1 on error), export, lock
    ///
    /// descriptor/module.yaml owns identity, requires, links, errors, i18n and
    /// permissions; the entity macros own fields and rules; descriptor.lock keeps proto
    /// tags stable; descriptor/module.descriptor.json is the materialized result.
    Descriptor(DescriptorCommand),

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

    /// Add an event flow pipeline to config/events.yaml
    EventFlow(AddEventFlowArgs),

    /// Add an event sink (delivery target) to config/events.yaml
    Sink(AddSinkArgs),

    /// Require another module from its manifest (descriptor JSON/YAML)
    Module(AddModuleArgs),
}

/// `this adopt …` — préparer un projet **existant** à this-rs + obrain + IAM.
#[derive(clap::Args)]
pub struct AdoptCommand {
    #[command(subcommand)]
    pub command: AdoptCommands,
}

#[derive(Subcommand)]
pub enum AdoptCommands {
    /// Prepare an existing project to use obrain, the canonical identity and IAM
    Obrain(AdoptObrainArgs),
}

#[derive(clap::Args)]
pub struct AdoptObrainArgs {
    /// Project directory (default: the detected project root)
    #[arg(long, value_name = "DIR")]
    pub path: Option<std::path::PathBuf>,

    /// Show the wiring for OBSERVATION mode: denials are computed, logged and
    /// published, but nothing is blocked — the safe first step on a running
    /// service
    #[arg(long)]
    pub report_only: bool,
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

    /// Enable event system (EventBus, NotificationStore, SSE streaming, event flows)
    #[arg(long)]
    pub events: bool,

    /// Enable WAMI authentication (STS endpoints, JWT middleware, tenant isolation, GDPR erasure)
    #[arg(long)]
    pub auth: bool,

    /// Enable cognitive signal bridge (anomaly detection, co-change, stigmergy notifications)
    /// Implies --events. Works with any storage backend.
    #[arg(long)]
    pub cognitive: bool,
}

/// Arguments for `this add entity <name>`
#[derive(Parser)]
pub struct AddEntityArgs {
    /// Entity name (singular, snake_case, e.g. "product")
    pub name: String,

    /// Entity fields as "name:Type[:rule]*", comma-separated
    /// Example: --fields "sku:String,price:f64,description:Option<String>"
    /// Rules (validators, input transforms, query capabilities), optional @create/@update:
    /// --fields "email:String:required:email,age:Option<i32>:min_value(0):max_value(150)"
    /// required optional positive string_length(a,b) min_value(n) max_value(n) in_list(a|b)
    /// date_format(f) pattern(re) email url trim uppercase lowercase round_decimals(n)
    /// filter(eq,gt,...) sortable read_only immutable
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

/// Arguments for `this add event-flow <name>`
#[derive(Parser)]
pub struct AddEventFlowArgs {
    /// Event flow name (e.g. "notify-on-create")
    pub name: String,

    /// Trigger pattern (e.g. "entity.created.*", "entity.updated.order")
    #[arg(long, default_value = "entity.created.*")]
    pub trigger: String,

    /// Delivery sink name to use in the final deliver step
    #[arg(long, default_value = "in-app")]
    pub sink: String,
}

/// Arguments for `this add sink <name>`
#[derive(Parser)]
pub struct AddSinkArgs {
    /// Sink name (e.g. "my-webhook")
    pub name: String,

    /// Sink type: in_app, webhook, push, websocket, counter
    #[arg(long, value_name = "TYPE")]
    pub sink_type: String,

    /// URL for webhook sinks
    #[arg(long)]
    pub url: Option<String>,
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

/// Arguments for `this info`
#[derive(Parser, Default)]
pub struct InfoArgs {
    /// Print the project information, materialized module descriptor included, as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Parser)]
pub struct DescriptorCommand {
    #[command(subcommand)]
    pub command: DescriptorCommands,
}

#[derive(Subcommand)]
pub enum DescriptorCommands {
    /// Enable descriptor mode: create descriptor/module.yaml and Module::descriptor()
    Init(DescriptorInitArgs),

    /// Verify module.yaml, its composition with the code, the lock (exit code 1 on error)
    Check(DescriptorCheckArgs),

    /// Write descriptor/module.descriptor.json, or derive another format
    Export(DescriptorExportArgs),

    /// Bring descriptor.lock up to date (new fields get tags, removed ones are reserved)
    Lock,
}

#[derive(Parser)]
pub struct DescriptorInitArgs {
    /// Module version (default: Module::version() of src/module.rs, else 1.0.0)
    #[arg(long = "module-version")]
    pub module_version: Option<String>,
}

#[derive(Parser)]
pub struct DescriptorCheckArgs {
    /// Machine-readable report
    #[arg(long)]
    pub json: bool,

    /// Treat warnings (stale lock or JSON) as errors
    #[arg(long)]
    pub strict: bool,
}

#[derive(Parser)]
pub struct DescriptorExportArgs {
    /// json (default: writes descriptor/module.descriptor.json), yaml, graphql, proto, json-schema
    #[arg(long, default_value = "json")]
    pub format: String,

    /// Write to this file instead
    #[arg(long)]
    pub output: Option<std::path::PathBuf>,

    /// Print to stdout instead of writing
    #[arg(long)]
    pub stdout: bool,
}

/// Arguments for `this add module <manifest>`
#[derive(Parser)]
pub struct AddModuleArgs {
    /// Manifest of the module: its descriptor (module.descriptor.json or YAML)
    pub manifest: std::path::PathBuf,

    /// Semver requirement (default: ^<manifest version>)
    #[arg(long = "version-req")]
    pub version_req: Option<String>,

    /// Optional dependency: loaded when present, never required
    #[arg(long)]
    pub optional: bool,
}
