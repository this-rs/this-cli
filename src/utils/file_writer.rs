use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

/// Abstraction for file system operations, enabling dry-run mode.
pub trait FileWriter {
    /// Create a directory and all parent directories
    fn create_dir_all(&self, path: &Path) -> Result<()>;

    /// Write content to a file (create or overwrite)
    fn write_file(&self, path: &Path, content: &str) -> Result<()>;

    /// Update an existing file (read + transform + write)
    /// In dry-run mode, shows a diff of what would change.
    fn update_file(&self, path: &Path, original: &str, updated: &str) -> Result<()>;

    /// Whether this is a dry-run (no actual writes)
    fn is_dry_run(&self) -> bool;
}

/// Real file writer — actually writes to disk
pub struct RealWriter;

impl FileWriter for RealWriter {
    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))
    }

    fn write_file(&self, path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write: {}", path.display()))
    }

    fn update_file(&self, path: &Path, _original: &str, updated: &str) -> Result<()> {
        std::fs::write(path, updated)
            .with_context(|| format!("Failed to write: {}", path.display()))
    }

    fn is_dry_run(&self) -> bool {
        false
    }
}

/// Dry-run writer — prints what would happen without writing
pub struct DryRunWriter {
    files_created: std::cell::RefCell<Vec<PathBuf>>,
    files_updated: std::cell::RefCell<Vec<PathBuf>>,
    dirs_created: std::cell::RefCell<Vec<PathBuf>>,
}

impl DryRunWriter {
    pub fn new() -> Self {
        Self {
            files_created: std::cell::RefCell::new(Vec::new()),
            files_updated: std::cell::RefCell::new(Vec::new()),
            dirs_created: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Print summary of what would be done
    pub fn print_summary(&self) {
        let created = self.files_created.borrow();
        let updated = self.files_updated.borrow();

        println!();
        if !created.is_empty() {
            println!(
                "  {} file(s) would be created",
                created.len().to_string().bold()
            );
        }
        if !updated.is_empty() {
            println!(
                "  {} file(s) would be modified",
                updated.len().to_string().bold()
            );
        }
        if created.is_empty() && updated.is_empty() {
            println!("  {}", "No changes would be made".dimmed());
        }
    }
}

impl FileWriter for DryRunWriter {
    fn create_dir_all(&self, path: &Path) -> Result<()> {
        self.dirs_created.borrow_mut().push(path.to_path_buf());
        Ok(())
    }

    fn write_file(&self, path: &Path, _content: &str) -> Result<()> {
        println!("  {} {}", "Would create:".cyan(), path.display());
        self.files_created.borrow_mut().push(path.to_path_buf());
        Ok(())
    }

    fn update_file(&self, path: &Path, original: &str, updated: &str) -> Result<()> {
        println!("  {} {}", "Would modify:".yellow(), path.display());
        self.files_updated.borrow_mut().push(path.to_path_buf());

        // Show a simplified diff
        print_simple_diff(original, updated);

        Ok(())
    }

    fn is_dry_run(&self) -> bool {
        true
    }
}

/// Print a simplified diff showing only added lines
fn print_simple_diff(original: &str, updated: &str) {
    let original_lines: Vec<&str> = original.lines().collect();
    let updated_lines: Vec<&str> = updated.lines().collect();

    for line in &updated_lines {
        if !original_lines.contains(line) {
            println!("    {} {}", "+".green(), line.green());
        }
    }
}
