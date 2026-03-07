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
            "build_project" => handle_build_project(&args),
            "start_dev" => handle_start_dev(&args),
            "add_target" => handle_add_target(&args),
            "add_event_flow" => handle_add_event_flow(&args),
            "add_sink" => handle_add_sink(&args),
            "generate_client" => handle_generate_client(&args),
            _ => anyhow::bail!("Unknown tool: {}", name),
        }
    }
}

/// RAII guard that restores the working directory on drop
#[derive(Debug)]
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

use crate::commands::{
    AddEntityArgs, AddEventFlowArgs, AddLinkArgs, AddSinkArgs, AddTargetArgs, BuildArgs, DevArgs,
    InitArgs,
};
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

    let workspace = args
        .get("workspace")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let websocket = args
        .get("websocket")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let grpc = args.get("grpc").and_then(|v| v.as_bool()).unwrap_or(false);

    let events = args
        .get("events")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let init_args = InitArgs {
        name: name.clone(),
        path: path.clone(),
        no_git,
        port,
        this_path: None,
        workspace,
        websocket,
        grpc,
        events,
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
        "websocket_enabled": websocket,
        "grpc_enabled": grpc,
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

    let backend = args
        .get("backend")
        .and_then(|v| v.as_str())
        .unwrap_or("in-memory")
        .to_string();

    let entity_args = AddEntityArgs {
        name: name.clone(),
        fields,
        validated,
        indexed,
        backend,
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

fn handle_build_project(args: &Value) -> Result<Value> {
    let embed = args.get("embed").and_then(|v| v.as_bool()).unwrap_or(false);

    let api_only = args
        .get("api_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let front_only = args
        .get("front_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let docker = args
        .get("docker")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let release = args
        .get("release")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let target = args
        .get("target")
        .and_then(|v| v.as_str())
        .map(String::from);

    let build_args = BuildArgs {
        embed,
        api_only,
        front_only,
        docker,
        release,
        target,
    };

    let mode = if embed {
        "embed"
    } else if api_only {
        "api_only"
    } else if front_only {
        "front_only"
    } else if docker {
        "docker"
    } else {
        "default"
    };

    crate::commands::build::run(build_args, &writer)?;

    Ok(serde_json::json!({
        "status": "success",
        "mode": mode,
        "files_created": writer.files_created(),
        "files_modified": writer.files_modified(),
    }))
}

fn handle_start_dev(args: &Value) -> Result<Value> {
    let api_only = args
        .get("api_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let no_watch = args
        .get("no_watch")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let port = args.get("port").and_then(|v| v.as_u64()).map(|p| p as u16);

    let _cwd_guard = CwdGuard::from_args(args)?;

    let dev_args = DevArgs {
        api_only,
        no_watch,
        port,
    };

    // Note: this dev is a long-running process. In MCP context, it will block
    // the stdio server. For now, we start it — the MCP client (e.g. Claude Code)
    // should run this in a background shell instead of via MCP for long-running use.
    // This handler is useful for validation (workspace detection, config parsing)
    // and for short-lived checks.
    crate::commands::dev::run(dev_args)?;

    Ok(serde_json::json!({
        "status": "stopped",
        "message": "Dev servers shut down",
    }))
}

fn handle_add_target(args: &Value) -> Result<Value> {
    let target_type_str = args
        .get("target_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: target_type"))?;

    let target_type: crate::config::TargetType = match target_type_str {
        "webapp" => crate::config::TargetType::Webapp,
        "website" => crate::config::TargetType::Website,
        "desktop" => crate::config::TargetType::Desktop,
        "ios" => crate::config::TargetType::Ios,
        "android" => crate::config::TargetType::Android,
        _ => anyhow::bail!(
            "Invalid target_type: '{}'. Must be one of: webapp, website, desktop, ios, android",
            target_type_str
        ),
    };

    let framework = args
        .get("framework")
        .and_then(|v| v.as_str())
        .unwrap_or("react")
        .to_string();

    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let target_args = AddTargetArgs {
        target_type: target_type.clone(),
        framework: framework.clone(),
        name,
    };

    crate::commands::add_target::run(target_args, &writer)?;

    Ok(serde_json::json!({
        "status": "success",
        "target_type": target_type_str,
        "framework": framework,
        "files_created": writer.files_created(),
        "files_modified": writer.files_modified(),
        "next_steps": ["cd front && npm install", "this dev"],
    }))
}

fn handle_add_event_flow(args: &Value) -> Result<Value> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
        .to_string();

    let trigger = args
        .get("trigger")
        .and_then(|v| v.as_str())
        .unwrap_or("entity.created.*")
        .to_string();

    let sink = args
        .get("sink")
        .and_then(|v| v.as_str())
        .unwrap_or("in-app")
        .to_string();

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let flow_args = AddEventFlowArgs {
        name: name.clone(),
        trigger: trigger.clone(),
        sink: sink.clone(),
    };

    crate::commands::add_event_flow::run(flow_args, &writer)?;

    Ok(serde_json::json!({
        "status": "success",
        "flow_name": name,
        "trigger": trigger,
        "sink": sink,
        "files_modified": writer.files_modified(),
    }))
}

fn handle_add_sink(args: &Value) -> Result<Value> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?
        .to_string();

    let sink_type = args
        .get("sink_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter: sink_type"))?
        .to_string();

    let url = args
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let sink_args = AddSinkArgs {
        name: name.clone(),
        sink_type: sink_type.clone(),
        url: url.clone(),
    };

    crate::commands::add_sink::run(sink_args, &writer)?;

    Ok(serde_json::json!({
        "status": "success",
        "sink_name": name,
        "sink_type": sink_type,
        "url": url,
        "files_modified": writer.files_modified(),
    }))
}

fn handle_generate_client(args: &Value) -> Result<Value> {
    let lang = args
        .get("lang")
        .and_then(|v| v.as_str())
        .unwrap_or("typescript")
        .to_string();

    if lang != "typescript" {
        anyhow::bail!(
            "Unsupported language: '{}'. Currently only 'typescript' is supported.",
            lang
        );
    }

    let output = args
        .get("output")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);

    let _cwd_guard = CwdGuard::from_args(args)?;
    let writer = McpFileWriter::new();

    let generate_args = crate::commands::GenerateClientArgs { lang, output };

    crate::commands::generate::run(generate_args, &writer)?;

    Ok(serde_json::json!({
        "status": "success",
        "lang": "typescript",
        "files_created": writer.files_created(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    // ── McpFileWriter tests ──────────────────────────────────────────

    #[test]
    fn test_mcp_file_writer_writes_and_tracks_created() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("hello.txt");

        let writer = McpFileWriter::new();
        writer.write_file(&file_path, "hello world").unwrap();

        // File exists on disk with correct content
        assert!(file_path.exists());
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "hello world");

        // Tracking records the created file
        let created = writer.files_created();
        assert_eq!(created.len(), 1);
        assert!(created[0].contains("hello.txt"));

        // Nothing modified yet
        assert!(writer.files_modified().is_empty());
    }

    #[test]
    fn test_mcp_file_writer_tracks_modified() {
        let tmp = TempDir::new().unwrap();
        let file_path = tmp.path().join("data.txt");

        // Pre-create the file
        std::fs::write(&file_path, "original").unwrap();

        let writer = McpFileWriter::new();
        writer
            .update_file(&file_path, "original", "updated")
            .unwrap();

        // Disk content is updated
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "updated");

        // Tracking records a modified file
        let modified = writer.files_modified();
        assert_eq!(modified.len(), 1);
        assert!(modified[0].contains("data.txt"));

        // Nothing created via write_file
        assert!(writer.files_created().is_empty());
    }

    #[test]
    fn test_mcp_file_writer_create_dir_all() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("a").join("b").join("c");

        let writer = McpFileWriter::new();
        writer.create_dir_all(&nested).unwrap();

        assert!(nested.exists());
        assert!(nested.is_dir());
    }

    #[test]
    fn test_mcp_file_writer_is_not_dry_run() {
        let writer = McpFileWriter::new();
        assert!(!writer.is_dry_run());
    }

    #[test]
    fn test_mcp_file_writer_multiple_writes() {
        let tmp = TempDir::new().unwrap();

        let writer = McpFileWriter::new();
        writer.write_file(&tmp.path().join("a.txt"), "aaa").unwrap();
        writer.write_file(&tmp.path().join("b.txt"), "bbb").unwrap();
        writer.write_file(&tmp.path().join("c.txt"), "ccc").unwrap();

        assert_eq!(writer.files_created().len(), 3);
    }

    // ── ToolHandler dispatch tests ───────────────────────────────────

    #[test]
    fn test_handle_unknown_tool() {
        let handler = ToolHandler::new();
        let result = handler.handle("nonexistent_tool", None);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Unknown tool"),
            "Error should mention 'Unknown tool', got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("nonexistent_tool"),
            "Error should mention the tool name, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_handle_with_none_args() {
        // Calling with None args should still work (defaults to empty object)
        let handler = ToolHandler::new();
        let result = handler.handle("init_project", None);

        // Should fail because "name" is required, not because of None args
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("name"));
    }

    #[test]
    #[serial]
    fn test_handle_init_project() {
        let tmp = TempDir::new().unwrap();
        let handler = ToolHandler::new();

        let args = serde_json::json!({
            "name": "test-project",
            "no_git": true,
            "cwd": tmp.path().to_str().unwrap()
        });

        let result = handler.handle("init_project", Some(args)).unwrap();

        // Check JSON response
        assert_eq!(result["status"], "success");
        assert_eq!(result["project_name"], "test-project");
        assert_eq!(result["project_path"], "test-project");
        assert_eq!(result["port"], 3000);

        // Verify files were actually created
        let project_dir = tmp.path().join("test-project");
        assert!(project_dir.exists(), "Project directory should exist");
        assert!(
            project_dir.join("Cargo.toml").exists(),
            "Cargo.toml should exist"
        );
        assert!(
            project_dir.join("src/main.rs").exists(),
            "src/main.rs should exist"
        );
        assert!(
            project_dir.join("src/entities/mod.rs").exists(),
            "src/entities/mod.rs should exist"
        );
        assert!(
            project_dir.join("config/links.yaml").exists(),
            "config/links.yaml should exist"
        );

        // Check files_created is populated
        let files_created = result["files_created"].as_array().unwrap();
        assert!(
            !files_created.is_empty(),
            "files_created should not be empty"
        );
    }

    #[test]
    #[serial]
    fn test_handle_init_project_with_custom_port() {
        let tmp = TempDir::new().unwrap();
        let handler = ToolHandler::new();

        let args = serde_json::json!({
            "name": "custom-port-project",
            "no_git": true,
            "port": 8080,
            "cwd": tmp.path().to_str().unwrap()
        });

        let result = handler.handle("init_project", Some(args)).unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["port"], 8080);
    }

    #[test]
    #[serial]
    fn test_handle_init_project_workspace_mode() {
        let tmp = TempDir::new().unwrap();
        let handler = ToolHandler::new();

        let args = serde_json::json!({
            "name": "ws-project",
            "no_git": true,
            "workspace": true,
            "cwd": tmp.path().to_str().unwrap()
        });

        let result = handler.handle("init_project", Some(args)).unwrap();
        assert_eq!(result["status"], "success");

        let ws_dir = tmp.path().join("ws-project");
        assert!(ws_dir.join("this.yaml").exists(), "this.yaml should exist");
        assert!(
            ws_dir.join("api/Cargo.toml").exists(),
            "api/Cargo.toml should exist"
        );
    }

    /// Helper: create a minimal this-rs project scaffold inside `dir` so that
    /// `detect_project_root()` will find it when CWD is set to `dir`.
    fn scaffold_project(dir: &std::path::Path) {
        // Create directory structure
        std::fs::create_dir_all(dir.join("src/entities")).unwrap();
        std::fs::create_dir_all(dir.join("config")).unwrap();

        // Cargo.toml with `this` dependency (required by detect_project_root)
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2024"

[dependencies]
this = { package = "this-rs", version = "0.0.6" }
"#,
        )
        .unwrap();

        // Empty entities/mod.rs
        std::fs::write(dir.join("src/entities/mod.rs"), "").unwrap();

        // Minimal stores.rs with markers
        std::fs::write(
            dir.join("src/stores.rs"),
            r#"use std::sync::Arc;

pub struct Stores {
    // [this:store_fields]
}

impl Stores {
    pub fn new_in_memory() -> Self {
        // [this:store_init_vars]

        Self {
            // [this:store_init_fields]
        }
    }
}
"#,
        )
        .unwrap();

        // Minimal module.rs with markers
        std::fs::write(
            dir.join("src/module.rs"),
            r#"use crate::stores::Stores;

pub struct AppModule {
    stores: Stores,
}

impl AppModule {
    fn entity_types(&self) -> Vec<&str> {
        vec![
            // [this:entity_types]
        ]
    }

    fn register_entities(&self, _registry: &mut EntityRegistry) {
        // [this:register_entities]
    }

    fn get_entity_fetcher(&self, _entity_type: &str) -> Option<Arc<dyn EntityStore>> {
        match _entity_type {
            // [this:entity_fetcher]
            _ => None,
        }
    }

    fn get_entity_creator(&self, _entity_type: &str) -> Option<Arc<dyn EntityStore>> {
        match _entity_type {
            // [this:entity_creator]
            _ => None,
        }
    }
}
"#,
        )
        .unwrap();

        // Minimal main.rs
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();

        // Minimal links.yaml
        std::fs::write(
            dir.join("config/links.yaml"),
            "entities: []\nlinks: []\nvalidation_rules: {}\n",
        )
        .unwrap();
    }

    #[test]
    #[serial]
    fn test_handle_add_entity() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("my-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "product",
            "fields": "sku:String,price:f64",
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("add_entity", Some(args)).unwrap();

        assert_eq!(result["status"], "success");
        assert_eq!(result["entity_name"], "product");

        // Entity files should exist
        let entity_dir = project_dir.join("src/entities/product");
        assert!(
            entity_dir.join("model.rs").exists(),
            "model.rs should exist"
        );
        assert!(
            entity_dir.join("store.rs").exists(),
            "store.rs should exist"
        );
        assert!(
            entity_dir.join("handlers.rs").exists(),
            "handlers.rs should exist"
        );
        assert!(entity_dir.join("mod.rs").exists(), "mod.rs should exist");

        // files_created should be populated
        let files_created = result["files_created"].as_array().unwrap();
        assert!(
            !files_created.is_empty(),
            "files_created should not be empty"
        );

        // files_modified should be populated (entities/mod.rs, stores.rs, module.rs, links.yaml)
        let files_modified = result["files_modified"].as_array().unwrap();
        assert!(
            !files_modified.is_empty(),
            "files_modified should not be empty"
        );
    }

    #[test]
    #[serial]
    fn test_handle_get_project_info() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("info-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("get_project_info", Some(args)).unwrap();

        // Should return structured project info
        assert_eq!(result["project_name"], "test-project");
        assert!(result.get("this_version").is_some());
        assert!(result.get("entities").is_some());
        assert!(result.get("links").is_some());
        assert!(result.get("coherence").is_some());
    }

    #[test]
    #[serial]
    fn test_handle_check_project_health() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("health-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("check_project_health", Some(args)).unwrap();

        // Should contain diagnostics array and summary
        assert!(result.get("diagnostics").is_some());
        let diagnostics = result["diagnostics"].as_array().unwrap();
        assert!(
            !diagnostics.is_empty(),
            "diagnostics should not be empty (at least Cargo.toml check)"
        );

        let summary = &result["summary"];
        assert!(summary.get("pass").is_some());
        assert!(summary.get("warn").is_some());
        assert!(summary.get("error").is_some());
        assert!(summary.get("total").is_some());

        // The total should match the array length
        let total = summary["total"].as_u64().unwrap() as usize;
        assert_eq!(total, diagnostics.len());
    }

    #[test]
    #[serial]
    fn test_handle_add_link() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("link-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        // First add two entities so links.yaml has known entities
        let handler = ToolHandler::new();

        let add_order = serde_json::json!({
            "name": "order",
            "cwd": project_dir.to_str().unwrap()
        });
        handler.handle("add_entity", Some(add_order)).unwrap();

        let add_invoice = serde_json::json!({
            "name": "invoice",
            "cwd": project_dir.to_str().unwrap()
        });
        handler.handle("add_entity", Some(add_invoice)).unwrap();

        // Now add a link
        let link_args = serde_json::json!({
            "source": "order",
            "target": "invoice",
            "cwd": project_dir.to_str().unwrap()
        });
        let result = handler.handle("add_link", Some(link_args)).unwrap();

        assert_eq!(result["status"], "success");
        assert_eq!(result["link"], "order -> invoice");

        let files_modified = result["files_modified"].as_array().unwrap();
        assert!(
            !files_modified.is_empty(),
            "files_modified should not be empty after adding a link"
        );
    }

    // ── Error handling tests ─────────────────────────────────────────

    #[test]
    fn test_handle_init_missing_name() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "no_git": true
        });

        let result = handler.handle("init_project", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("name"),
            "Error should mention 'name', got: {}",
            err
        );
    }

    #[test]
    #[serial]
    fn test_handle_add_entity_outside_project() {
        let tmp = TempDir::new().unwrap();
        // No project scaffold — just an empty directory

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "product",
            "cwd": tmp.path().to_str().unwrap()
        });

        let result = handler.handle("add_entity", Some(args));
        assert!(
            result.is_err(),
            "Should fail when not inside a this-rs project"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("this-rs project") || err.contains("Cargo.toml"),
            "Error should mention project detection failure, got: {}",
            err
        );
    }

    #[test]
    fn test_handle_add_link_missing_source() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "target": "invoice"
        });

        let result = handler.handle("add_link", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("source"),
            "Error should mention 'source', got: {}",
            err
        );
    }

    #[test]
    fn test_handle_add_link_missing_target() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "source": "order"
        });

        let result = handler.handle("add_link", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("target"),
            "Error should mention 'target', got: {}",
            err
        );
    }

    #[test]
    fn test_handle_add_target_missing_target_type() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({});

        let result = handler.handle("add_target", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("target_type"),
            "Error should mention 'target_type', got: {}",
            err
        );
    }

    #[test]
    fn test_handle_add_target_invalid_target_type() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "target_type": "invalid_platform"
        });

        let result = handler.handle("add_target", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid target_type"),
            "Error should mention 'Invalid target_type', got: {}",
            err
        );
    }

    #[test]
    fn test_handle_generate_client_unsupported_language() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "lang": "python"
        });

        let result = handler.handle("generate_client", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unsupported language"),
            "Error should mention 'Unsupported language', got: {}",
            err
        );
    }

    #[test]
    #[serial]
    fn test_handle_get_project_info_outside_project() {
        let tmp = TempDir::new().unwrap();
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "cwd": tmp.path().to_str().unwrap()
        });

        let result = handler.handle("get_project_info", Some(args));
        assert!(
            result.is_err(),
            "Should fail when not inside a this-rs project"
        );
    }

    #[test]
    #[serial]
    fn test_handle_check_project_health_outside_project() {
        let tmp = TempDir::new().unwrap();
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "cwd": tmp.path().to_str().unwrap()
        });

        let result = handler.handle("check_project_health", Some(args));
        assert!(
            result.is_err(),
            "Should fail when not inside a this-rs project"
        );
    }

    #[test]
    #[serial]
    fn test_cwd_guard_changes_and_restores_directory() {
        let original_cwd = std::env::current_dir().unwrap();
        let tmp = TempDir::new().unwrap();

        {
            let args = serde_json::json!({
                "cwd": tmp.path().to_str().unwrap()
            });
            let _guard = CwdGuard::from_args(&args).unwrap();
            // Inside the guard scope, CWD should be the tmp dir
            let current = std::env::current_dir().unwrap();
            assert_eq!(
                current.canonicalize().unwrap(),
                tmp.path().canonicalize().unwrap()
            );
        }

        // After the guard is dropped, CWD should be restored
        let restored = std::env::current_dir().unwrap();
        assert_eq!(
            restored.canonicalize().unwrap(),
            original_cwd.canonicalize().unwrap()
        );
    }

    #[test]
    fn test_cwd_guard_no_cwd_field() {
        let args = serde_json::json!({
            "name": "test"
        });

        let guard = CwdGuard::from_args(&args).unwrap();
        // original should be None when no cwd field is present
        assert!(guard.original.is_none());
    }

    #[test]
    fn test_cwd_guard_invalid_directory() {
        let args = serde_json::json!({
            "cwd": "/nonexistent/path/that/does/not/exist"
        });

        let result = CwdGuard::from_args(&args);
        assert!(result.is_err(), "Should fail with a non-existent directory");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Failed to change to directory"),
            "Error should mention directory change failure, got: {}",
            err
        );
    }

    #[test]
    #[serial]
    fn test_handle_init_project_duplicate_name() {
        let tmp = TempDir::new().unwrap();
        let handler = ToolHandler::new();

        let args = serde_json::json!({
            "name": "dup-project",
            "no_git": true,
            "cwd": tmp.path().to_str().unwrap()
        });

        // First init should succeed
        let result = handler.handle("init_project", Some(args.clone())).unwrap();
        assert_eq!(result["status"], "success");

        // Second init with same name should fail (directory already exists)
        let result2 = handler.handle("init_project", Some(args));
        assert!(
            result2.is_err(),
            "Should fail when project directory already exists"
        );
        let err = result2.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention directory already exists, got: {}",
            err
        );
    }

    // ── Event flow & sink handler tests ─────────────────────────────

    #[test]
    fn test_handle_add_event_flow_missing_name() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "trigger": "entity.created.*",
            "sink": "in-app"
        });

        let result = handler.handle("add_event_flow", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("name"),
            "Error should mention 'name', got: {}",
            err
        );
    }

    #[test]
    #[serial]
    fn test_handle_add_event_flow_success() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("flow-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        // Add events.yaml with a sink
        std::fs::create_dir_all(project_dir.join("config")).unwrap();
        std::fs::write(
            project_dir.join("config/events.yaml"),
            "event_sinks:\n  - name: in-app\n    type: in_app\nevent_flows: []\n",
        )
        .unwrap();

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "notify-on-create",
            "trigger": "entity.created.*",
            "sink": "in-app",
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("add_event_flow", Some(args)).unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["flow_name"], "notify-on-create");
        assert_eq!(result["trigger"], "entity.created.*");
        assert_eq!(result["sink"], "in-app");
    }

    #[test]
    #[serial]
    fn test_handle_add_event_flow_default_trigger_and_sink() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("flow-defaults");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        std::fs::create_dir_all(project_dir.join("config")).unwrap();
        std::fs::write(
            project_dir.join("config/events.yaml"),
            "event_sinks:\n  - name: in-app\n    type: in_app\nevent_flows: []\n",
        )
        .unwrap();

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "default-flow",
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("add_event_flow", Some(args)).unwrap();
        assert_eq!(result["status"], "success");
        // Defaults: trigger = "entity.created.*", sink = "in-app"
        assert_eq!(result["trigger"], "entity.created.*");
        assert_eq!(result["sink"], "in-app");
    }

    #[test]
    fn test_handle_add_sink_missing_name() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "sink_type": "in_app"
        });

        let result = handler.handle("add_sink", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("name"),
            "Error should mention 'name', got: {}",
            err
        );
    }

    #[test]
    fn test_handle_add_sink_missing_sink_type() {
        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "my-sink"
        });

        let result = handler.handle("add_sink", Some(args));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("sink_type"),
            "Error should mention 'sink_type', got: {}",
            err
        );
    }

    #[test]
    #[serial]
    fn test_handle_add_sink_success() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("sink-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        std::fs::create_dir_all(project_dir.join("config")).unwrap();
        std::fs::write(
            project_dir.join("config/events.yaml"),
            "event_sinks: []\nevent_flows: []\n",
        )
        .unwrap();

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "my-webhook",
            "sink_type": "webhook",
            "url": "https://example.com/hook",
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("add_sink", Some(args)).unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["sink_name"], "my-webhook");
        assert_eq!(result["sink_type"], "webhook");
        assert_eq!(result["url"], "https://example.com/hook");
    }

    #[test]
    #[serial]
    fn test_handle_add_sink_in_app_no_url() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("sink-nourl");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        std::fs::create_dir_all(project_dir.join("config")).unwrap();
        std::fs::write(
            project_dir.join("config/events.yaml"),
            "event_sinks: []\nevent_flows: []\n",
        )
        .unwrap();

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "notif",
            "sink_type": "in_app",
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("add_sink", Some(args)).unwrap();
        assert_eq!(result["status"], "success");
        assert_eq!(result["sink_name"], "notif");
        assert_eq!(result["sink_type"], "in_app");
        assert!(result["url"].is_null());
    }

    #[test]
    #[serial]
    fn test_handle_add_entity_with_validation() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("validated-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "product",
            "fields": "sku:String,price:f64",
            "validated": true,
            "cwd": project_dir.to_str().unwrap()
        });

        let result = handler.handle("add_entity", Some(args)).unwrap();
        assert_eq!(result["status"], "success");

        // model.rs should exist
        let model_path = project_dir.join("src/entities/product/model.rs");
        assert!(model_path.exists());
    }

    #[test]
    #[serial]
    fn test_handle_add_entity_duplicate() {
        let tmp = TempDir::new().unwrap();
        let project_dir = tmp.path().join("dup-entity-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        scaffold_project(&project_dir);

        let handler = ToolHandler::new();
        let args = serde_json::json!({
            "name": "widget",
            "cwd": project_dir.to_str().unwrap()
        });

        // First add should succeed
        let result = handler.handle("add_entity", Some(args.clone())).unwrap();
        assert_eq!(result["status"], "success");

        // Second add with same entity name should fail
        let result2 = handler.handle("add_entity", Some(args));
        assert!(result2.is_err(), "Should fail when entity already exists");
        let err = result2.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention entity already exists, got: {}",
            err
        );
    }
}
