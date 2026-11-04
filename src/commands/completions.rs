use std::io::{self, Write};

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{Shell, generate};

use super::Cli;

pub fn run(shell: Shell) -> Result<()> {
    run_to(shell, &mut io::stdout())
}

/// Write the completion script to `out`.
///
/// Taking the writer as a parameter keeps the tests off the process stdout,
/// which other tests may share with child processes (macOS shares the
/// non-blocking flag across an inherited descriptor, so writing to it can
/// fail with `WouldBlock`).
pub fn run_to(shell: Shell, out: &mut dyn Write) -> Result<()> {
    let mut cmd = Cli::command();
    let bin_name = "this".to_string();
    generate(shell, &mut cmd, bin_name, out);
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(shell: Shell) -> String {
        let mut buf = Vec::new();
        run_to(shell, &mut buf).expect("completions should be generated");
        String::from_utf8(buf).expect("completion script is UTF-8")
    }

    #[test]
    fn test_completions_bash() {
        assert!(script(Shell::Bash).contains("this"));
    }

    #[test]
    fn test_completions_zsh() {
        assert!(script(Shell::Zsh).contains("this"));
    }

    #[test]
    fn test_completions_fish() {
        assert!(script(Shell::Fish).contains("this"));
    }

    #[test]
    fn test_completions_powershell() {
        assert!(script(Shell::PowerShell).contains("this"));
    }
}
