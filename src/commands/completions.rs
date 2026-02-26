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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completions_bash() {
        let result = run(Shell::Bash);
        assert!(result.is_ok(), "Bash completions should succeed");
    }

    #[test]
    fn test_completions_zsh() {
        let result = run(Shell::Zsh);
        assert!(result.is_ok(), "Zsh completions should succeed");
    }

    #[test]
    fn test_completions_fish() {
        let result = run(Shell::Fish);
        assert!(result.is_ok(), "Fish completions should succeed");
    }

    #[test]
    fn test_completions_powershell() {
        let result = run(Shell::PowerShell);
        assert!(result.is_ok(), "PowerShell completions should succeed");
    }
}
