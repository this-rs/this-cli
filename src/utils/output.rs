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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_banner() {
        // Smoke test — should not panic
        print_banner();
    }

    #[test]
    fn test_print_step() {
        print_step("step");
    }

    #[test]
    fn test_print_file_created() {
        print_file_created("file.rs");
    }

    #[test]
    fn test_print_success() {
        print_success("done");
    }

    #[test]
    fn test_print_error() {
        print_error("oops");
    }

    #[test]
    fn test_print_info() {
        print_info("info");
    }

    #[test]
    fn test_print_warn() {
        print_warn("warning");
    }

    #[test]
    fn test_print_next_steps() {
        print_next_steps(&["step 1", "step 2", "step 3"]);
    }
}
