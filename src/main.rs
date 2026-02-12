mod commands;
mod config;
mod mcp;
mod templates;
mod utils;

use clap::Parser;
use commands::{AddCommands, Cli, Commands};
use utils::file_writer::{DryRunWriter, RealWriter};
use utils::output;

fn main() {
    let cli = Cli::parse();

    let dry_run = cli.dry_run;

    let result = if dry_run {
        let writer = DryRunWriter::new();
        let res = run_command(cli, &writer);
        writer.print_summary();
        res
    } else {
        let writer = RealWriter;
        run_command(cli, &writer)
    };

    if let Err(e) = result {
        output::print_error(&format!("{:#}", e));
        std::process::exit(1);
    }
}

fn run_command(cli: Cli, writer: &dyn utils::file_writer::FileWriter) -> anyhow::Result<()> {
    match cli.command {
        Commands::Init(args) => commands::init::run(args, writer),
        Commands::Add(add) => match add.command {
            AddCommands::Entity(args) => commands::add_entity::run(args, writer),
            AddCommands::Link(args) => commands::add_link::run(args, writer),
        },
        Commands::Info => commands::info::run(),
        Commands::Doctor => commands::doctor::run(),
        Commands::Completions { shell } => commands::completions::run(shell),
        Commands::Build(args) => commands::build::run(args, writer),
        Commands::Dev(args) => commands::dev::run(args),
        Commands::Mcp => {
            let mut server = mcp::server::McpServer::new();
            server.run()
        }
    }
}
