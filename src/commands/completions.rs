use std::io;

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{Shell, generate};

use super::Cli;

pub fn run(shell: Shell) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = "this".to_string();
    generate(shell, &mut cmd, bin_name, &mut io::stdout());
    Ok(())
}
