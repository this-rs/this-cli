use colored::Colorize;

/// Print the this-rs banner
pub fn print_banner() {
    println!(
        "{}",
        r#"
  ┌─────────────────────────────┐
  │   this-rs project builder   │
  └─────────────────────────────┘"#
            .cyan()
    );
    println!();
}

/// Print a step in progress
pub fn print_step(msg: &str) {
    println!("✨ {}", msg.bold());
}

/// Print a file creation event
pub fn print_file_created(path: &str) {
    println!("  📄 {}", path.dimmed());
}

/// Print a success message
pub fn print_success(msg: &str) {
    println!("✅ {}", msg.green().bold());
}

/// Print an error message
pub fn print_error(msg: &str) {
    eprintln!("❌ {}", msg.red().bold());
}

/// Print an informational message
pub fn print_info(msg: &str) {
    println!("  📝 {}", msg);
}

/// Print a warning message
pub fn print_warn(msg: &str) {
    println!("  ⚠️ {}", msg.yellow());
}

/// Print next steps instructions
pub fn print_next_steps(steps: &[&str]) {
    println!();
    println!("{}", "Next steps:".bold());
    for step in steps {
        println!("  {}", step);
    }
}
