pub mod add_entity;
pub mod add_link;
pub mod completions;
pub mod doctor;
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
