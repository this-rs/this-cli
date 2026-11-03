//! Human-readable output for the CLI, on a channel that depends on the mode.
//!
//! # Two channels, one writer
//!
//! In normal CLI use stdout belongs to the human: `this info`, `this init` and
//! friends are read in a terminal, so that is where the text goes.
//!
//! In `this mcp` mode stdout belongs to the **protocol**: the JSON-RPC server
//! writes one message per line there and a client parses that stream. Any other
//! line corrupts it. (Measured before this module was introduced, `this-cli`
//! 0.0.5: one `tools/call init_project` produced 24 stdout lines, 2 responses
//! and 19 human lines.) So in MCP mode every human line goes to stderr.
//!
//! # Why a mode instead of "always stderr"
//!
//! Moving the CLI output to stderr unconditionally would break `this info
//! --json` and `this descriptor export` piped into `jq` — real, supported uses.
//! The mode keeps CLI behaviour byte-for-byte identical and only redirects when
//! stdout is not ours to use.

use colored::Colorize;
use std::io::Write;
use std::sync::OnceLock;

/// Where human-readable output goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutputMode {
    /// Normal CLI use: humans read stdout.
    Cli,
    /// MCP server mode: stdout carries JSON-RPC, humans read stderr.
    Mcp,
}

/// Set once in `main`, before anything is printed. Never mutated afterwards.
static MODE: OnceLock<OutputMode> = OnceLock::new();

/// Select the output mode. The first call wins; later calls are ignored.
pub fn set_mode(mode: OutputMode) {
    let _ = MODE.set(mode);
}

/// The mode in effect (CLI unless [`set_mode`] said otherwise).
pub fn mode() -> OutputMode {
    *MODE.get().unwrap_or(&OutputMode::Cli)
}

/// Print one line on the human channel.
pub fn line(msg: impl AsRef<str>) {
    emit(true, msg.as_ref());
}

/// Print an empty line on the human channel.
pub fn blank() {
    emit(true, "");
}

/// Print on the human channel without a trailing newline (use [`line`] or
/// [`blank`] to terminate).
pub fn inline(msg: impl AsRef<str>) {
    emit(false, msg.as_ref());
}

fn emit(newline: bool, msg: &str) {
    // Errors on the channel (EPIPE on a closed pipe, typically) are dropped:
    // a CLI that cannot write its own output has nothing useful left to say.
    match mode() {
        OutputMode::Cli => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            let _ = if newline {
                writeln!(lock, "{msg}")
            } else {
                write!(lock, "{msg}")
            };
            let _ = lock.flush();
        }
        OutputMode::Mcp => {
            let stderr = std::io::stderr();
            let mut lock = stderr.lock();
            let _ = if newline {
                writeln!(lock, "{msg}")
            } else {
                write!(lock, "{msg}")
            };
            let _ = lock.flush();
        }
    }
}

/// Drop-in replacement for `println!` for human-facing text.
///
/// Use it instead of `println!` anywhere the text is meant for a person: it
/// follows [`OutputMode`], so the MCP channel stays clean. Machine output that
/// must reach stdout in CLI mode (JSON for `jq`) goes through this too — in MCP
/// mode there is no legitimate machine consumer of stdout.
macro_rules! human {
    () => {
        $crate::utils::output::blank()
    };
    ($($arg:tt)*) => {
        $crate::utils::output::line(format!($($arg)*))
    };
}

pub(crate) use human;

/// Print the this-rs banner
pub fn print_banner() {
    line(
        r#"
  ┌─────────────────────────────┐
  │   this-rs project builder   │
  └─────────────────────────────┘"#
            .cyan()
            .to_string(),
    );
    blank();
}

/// Print a step in progress
pub fn print_step(msg: &str) {
    line(format!("✨ {}", msg.bold()));
}

/// Print a file creation event
pub fn print_file_created(path: &str) {
    line(format!("  📄 {}", path.dimmed()));
}

/// Print a success message
pub fn print_success(msg: &str) {
    line(format!("✅ {}", msg.green().bold()));
}

/// Print an error message
///
/// Errors always go to stderr, in every mode: that is where a caller expects
/// them and where they stay out of both the CLI's stdout and the MCP protocol.
pub fn print_error(msg: &str) {
    eprintln!("❌ {}", msg.red().bold());
}

/// Print an informational message
pub fn print_info(msg: &str) {
    line(format!("  📝 {}", msg));
}

/// Print a warning message
pub fn print_warn(msg: &str) {
    line(format!("  ⚠️ {}", msg.yellow()));
}

/// Print next steps instructions
pub fn print_next_steps(steps: &[&str]) {
    blank();
    line("Next steps:".bold().to_string());
    for step in steps {
        line(format!("  {}", step));
    }
}
