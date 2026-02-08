mod commands;
mod templates;
mod utils;

use clap::Parser;
use commands::{AddCommands, Cli, Commands};
use utils::output;

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init(args) => commands::init::run(args),
        Commands::Add(add) => match add.command {
            AddCommands::Entity(args) => commands::add_entity::run(args),
            AddCommands::Link(args) => commands::add_link::run(args),
        },
    };

    if let Err(e) = result {
        output::print_error(&format!("{:#}", e));
        std::process::exit(1);
    }
}
