//! MCP tool handlers — bridge between MCP tool calls and CLI commands

use anyhow::Result;
use serde_json::Value;

/// Handles MCP tool calls by dispatching to the appropriate CLI command
pub struct ToolHandler;

impl ToolHandler {
    pub fn new() -> Self {
        Self
    }

    /// Handle a tool call by name with the given arguments
    pub fn handle(&self, name: &str, args: Option<Value>) -> Result<Value> {
        let args = args.unwrap_or(Value::Object(serde_json::Map::new()));

        match name {
            "init_project" => handle_init_project(&args),
            "add_entity" => handle_add_entity(&args),
            "add_link" => handle_add_link(&args),
            "get_project_info" => handle_get_project_info(&args),
            "check_project_health" => handle_check_project_health(&args),
            _ => anyhow::bail!("Unknown tool: {}", name),
        }
    }
}

/// RAII guard that restores the working directory on drop
struct CwdGuard {
    original: Option<std::path::PathBuf>,
}

impl CwdGuard {
    /// If args contains a "cwd" field, change to that directory and return a guard
    /// that will restore the original CWD on drop.
    fn from_args(args: &Value) -> Result<Self> {
        if let Some(cwd) = args.get("cwd").and_then(|v| v.as_str()) {
            let original = std::env::current_dir().ok();
            std::env::set_current_dir(cwd)
                .map_err(|e| anyhow::anyhow!("Failed to change to directory '{}': {}", cwd, e))?;
            Ok(Self { original })
        } else {
            Ok(Self { original: None })
        }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        if let Some(ref original) = self.original {
            let _ = std::env::set_current_dir(original);
        }
    }
}

use crate::commands::{AddEntityArgs, AddLinkArgs, InitArgs};
use crate::utils::file_writer::FileWriter;

/// FileWriter that performs real operations AND tracks created/modified files
pub struct McpFileWriter {
    files_created: std::cell::RefCell<Vec<std::path::PathBuf>>,
    files_modified: std::cell::RefCell<Vec<std::path::PathBuf>>,
}

impl McpFileWriter {
    pub fn new() -> Self {
        Self {
            files_created: std::cell::RefCell::new(Vec::new()),
            files_modified: std::cell::RefCell::new(Vec::new()),
        }
    }

    pub fn files_created(&self) -> Vec<String> {
        self.files_created
            .borrow()
            .iter()
            .map(|p| p.display().to_string())
            .collect()
    }

    pub fn files_modified(&self) -> Vec<String> {
        self.files_modified
            .borrow()
            .iter()
            .map(|p| p.display().to_string())
            .collect()
    }
}

impl FileWriter for McpFileWriter {
    fn create_dir_all(&self, path: &std::path::Path) -> Result<()> {
        std::fs::create_dir_all(path)
            .map_err(|e| anyhow::anyhow!("Failed to create directory '{}': {}", path.display(), e))
    }

    fn write_file(&self, path: &std::path::Path, content: &str) -> Result<()> {
        std::fs::write(path, content)
            .map_err(|e| anyhow::anyhow!("Failed to write '{}': {}", path.display(), e))?;
        self.files_created.borrow_mut().push(path.to_path_buf());
        Ok(())
    }

    fn update_file(&self, path: &std::path::Path, _original: &str, updated: &str) -> Result<()> {
        std::fs::write(path, updated)
            .map_err(|e| anyhow::anyhow!("Failed to write '{}': {}", path.display(), e))?;
        self.files_modified.borrow_mut().push(path.to_path_buf());
        Ok(())
    }

    fn is_dry_run(&self) -> bool {
        false
    }
}

fn handle_init_project(args: &Value) -> Result<Value> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
        .to_string();

    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".")
        .to_string();

    let no_git = args
        .get("no_git")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let port = args.get("port").and_then(|v| v.as_u64()).unwrap_or(3000) as u16;

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let init_args = InitArgs {
        name: name.clone(),
        path: path.clone(),
        no_git,
        port,
        this_path: None,
    };

    crate::commands::init::run(init_args, &writer)?;

    let project_path = if path == "." {
        name.clone()
    } else {
        format!("{}/{}", path, name)
    };

    Ok(serde_json::json!({
        "status": "success",
        "project_name": name,
        "project_path": project_path,
        "port": port,
        "files_created": writer.files_created(),
    }))
}

fn handle_add_entity(args: &Value) -> Result<Value> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
        .to_string();

    let fields = args
        .get("fields")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let validated = args
        .get("validated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let indexed = args
        .get("indexed")
        .and_then(|v| v.as_str())
        .unwrap_or("name")
        .to_string();

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let entity_args = AddEntityArgs {
        name: name.clone(),
        fields,
        validated,
        indexed,
    };

    crate::commands::add_entity::run(entity_args, &writer)?;

    Ok(serde_json::json!({
        "status": "success",
        "entity_name": name,
        "files_created": writer.files_created(),
        "files_modified": writer.files_modified(),
    }))
}

fn handle_add_link(args: &Value) -> Result<Value> {
    let source = args
        .get("source")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: source"))?
        .to_string();

    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: target"))?
        .to_string();

    let link_type = args
        .get("link_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let forward = args
        .get("forward")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let reverse = args
        .get("reverse")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let no_validation_rule = args
        .get("no_validation_rule")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let link_args = AddLinkArgs {
        source: source.clone(),
        target: target.clone(),
        link_type,
        forward,
        reverse,
        description,
        no_validation_rule,
    };

    crate::commands::add_link::run(link_args, &writer)?;

    Ok(serde_json::json!({
        "status": "success",
        "link": format!("{} -> {}", source, target),
        "files_modified": writer.files_modified(),
    }))
}

fn handle_get_project_info(args: &Value) -> Result<Value> {
    let _cwd_guard = CwdGuard::from_args(args)?;

    // info::run() prints to stdout — we capture it for structured JSON
    let info = crate::commands::info::collect_info()?;
    Ok(serde_json::to_value(info)?)
}

fn handle_check_project_health(args: &Value) -> Result<Value> {
    let _cwd_guard = CwdGuard::from_args(args)?;

    let diagnostics = crate::commands::doctor::collect_diagnostics()?;

    let pass = diagnostics.iter().filter(|d| d.level == "pass").count();
    let warn = diagnostics.iter().filter(|d| d.level == "warn").count();
    let error = diagnostics.iter().filter(|d| d.level == "error").count();

    Ok(serde_json::json!({
        "diagnostics": diagnostics,
        "summary": {
            "pass": pass,
            "warn": warn,
            "error": error,
            "total": diagnostics.len(),
        }
    }))
}
