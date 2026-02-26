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

#[allow(dead_code)]
impl DryRunWriter {
    pub fn new() -> Self {
        Self {
            files_created: std::cell::RefCell::new(Vec::new()),
            files_updated: std::cell::RefCell::new(Vec::new()),
            dirs_created: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Get the list of files that would be created
    pub fn files_created(&self) -> Vec<PathBuf> {
        self.files_created.borrow().clone()
    }

    /// Get the list of files that would be updated
    pub fn files_updated(&self) -> Vec<PathBuf> {
        self.files_updated.borrow().clone()
    }

    /// Get the list of directories that would be created
    pub fn dirs_created(&self) -> Vec<PathBuf> {
        self.dirs_created.borrow().clone()
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── RealWriter tests ────────────────────────────────────────────────

    #[test]
    fn test_real_writer_write_file() {
        let tmp = TempDir::new().unwrap();
        let writer = RealWriter;
        let file = tmp.path().join("hello.txt");

        writer.write_file(&file, "hello world").unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_real_writer_create_dir_all() {
        let tmp = TempDir::new().unwrap();
        let writer = RealWriter;
        let nested = tmp.path().join("a").join("b").join("c");

        writer.create_dir_all(&nested).unwrap();

        assert!(nested.is_dir());
    }

    #[test]
    fn test_real_writer_update_file() {
        let tmp = TempDir::new().unwrap();
        let writer = RealWriter;
        let file = tmp.path().join("data.txt");

        writer.write_file(&file, "version 1").unwrap();
        writer.update_file(&file, "version 1", "version 2").unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "version 2");
    }

    #[test]
    fn test_real_writer_is_not_dry_run() {
        let writer = RealWriter;
        assert!(!writer.is_dry_run());
    }

    // ── DryRunWriter tests ──────────────────────────────────────────────

    #[test]
    fn test_dry_run_writer_tracks_files_created() {
        let writer = DryRunWriter::new();
        let path = PathBuf::from("/fake/new_file.rs");

        writer.write_file(&path, "content").unwrap();

        assert_eq!(writer.files_created(), vec![path]);
    }

    #[test]
    fn test_dry_run_writer_tracks_files_updated() {
        let writer = DryRunWriter::new();
        let path = PathBuf::from("/fake/existing.rs");

        writer.update_file(&path, "old", "new").unwrap();

        assert_eq!(writer.files_updated(), vec![path]);
    }

    #[test]
    fn test_dry_run_writer_tracks_dirs_created() {
        let writer = DryRunWriter::new();
        let path = PathBuf::from("/fake/some/dir");

        writer.create_dir_all(&path).unwrap();

        assert_eq!(writer.dirs_created(), vec![path]);
    }

    #[test]
    fn test_dry_run_writer_is_dry_run() {
        let writer = DryRunWriter::new();
        assert!(writer.is_dry_run());
    }

    #[test]
    fn test_dry_run_writer_does_not_write_real_files() {
        let tmp = TempDir::new().unwrap();
        let writer = DryRunWriter::new();
        let file = tmp.path().join("should_not_exist.txt");

        writer.write_file(&file, "content").unwrap();

        assert!(!file.exists(), "DryRunWriter must not create real files");
    }

    #[test]
    fn test_print_summary_no_changes() {
        let writer = DryRunWriter::new();
        // Should not panic when there are no operations
        writer.print_summary();
    }

    #[test]
    fn test_print_summary_with_changes() {
        let writer = DryRunWriter::new();
        writer.write_file(Path::new("/fake/a.rs"), "a").unwrap();
        writer.write_file(Path::new("/fake/b.rs"), "b").unwrap();
        writer
            .update_file(Path::new("/fake/c.rs"), "old", "new")
            .unwrap();

        // Should not panic with created + updated files
        writer.print_summary();
    }

    #[test]
    fn test_print_simple_diff() {
        // Should not panic — just prints to stdout
        print_simple_diff("line1\nline2\n", "line1\nline2\nline3\n");
    }
}
