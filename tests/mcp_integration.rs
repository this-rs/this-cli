//! Integration tests for the MCP server (`this mcp`)
//!
//! These tests spawn `this mcp` as a child process, send JSON-RPC messages
//! on stdin, and verify the responses on stdout.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn this_bin() -> String {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_target = manifest_dir.parent().unwrap().join("target/debug/this");
    if workspace_target.exists() {
        return workspace_target.to_string_lossy().to_string();
    }
    manifest_dir
        .join("target/debug/this")
        .to_string_lossy()
        .to_string()
}

/// Send JSON-RPC messages to `this mcp` and collect all responses
fn mcp_call(messages: &[&str]) -> Vec<Value> {
    let mut child = Command::new(this_bin())
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn this mcp");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    for msg in messages {
        writeln!(stdin, "{}", msg).expect("Failed to write to stdin");
    }
    drop(stdin); // Close stdin to signal EOF

    let stdout = child.stdout.take().expect("Failed to open stdout");
    let reader = BufReader::new(stdout);

    let responses: Vec<Value> = reader
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();

    child.wait().expect("Failed to wait for child");
    responses
}

/// Build a JSON-RPC request string
fn json_rpc(method: &str, params: Option<Value>, id: u64) -> String {
    let mut req = json!({
        "jsonrpc": "2.0",
        "method": method,
        "id": id,
    });
    if let Some(p) = params {
        req["params"] = p;
    }
    serde_json::to_string(&req).unwrap()
}

/// Standard initialize message
fn initialize_msg() -> String {
    json_rpc(
        "initialize",
        Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test", "version": "1.0"}
        })),
        1,
    )
}

// ============================================================================
// Protocol tests
// ============================================================================

#[test]
fn test_mcp_initialize() {
    let init = initialize_msg();
    let responses = mcp_call(&[&init]);

    assert_eq!(responses.len(), 1);
    let resp = &responses[0];
    assert_eq!(resp["id"], 1);
    assert!(resp["result"].is_object());
    assert_eq!(resp["result"]["serverInfo"]["name"], "this-cli");
    assert!(resp["result"]["serverInfo"]["version"].is_string());
    assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
    assert!(resp["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn test_mcp_tools_list() {
    let init = initialize_msg();
    let list = json_rpc("tools/list", None, 2);
    let responses = mcp_call(&[&init, &list]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    assert_eq!(resp["id"], 2);

    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"init_project"));
    assert!(tool_names.contains(&"add_entity"));
    assert!(tool_names.contains(&"add_link"));
    assert!(tool_names.contains(&"get_project_info"));
    assert!(tool_names.contains(&"check_project_health"));

    // Verify each tool has inputSchema
    for tool in tools {
        assert!(
            tool["inputSchema"].is_object(),
            "Tool {} missing inputSchema",
            tool["name"]
        );
        assert_eq!(tool["inputSchema"]["type"], "object");
    }
}

#[test]
fn test_mcp_invalid_json_error() {
    let responses = mcp_call(&["not json at all"]);

    assert_eq!(responses.len(), 1);
    let resp = &responses[0];
    assert!(resp["error"].is_object());
    assert_eq!(resp["error"]["code"], -32700); // Parse error
}

#[test]
fn test_mcp_method_not_found() {
    let init = initialize_msg();
    let unknown = json_rpc("unknown/method", None, 2);
    let responses = mcp_call(&[&init, &unknown]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    assert!(resp["error"].is_object());
    assert_eq!(resp["error"]["code"], -32601); // Method not found
}

#[test]
fn test_mcp_tools_list_before_init_error() {
    let list = json_rpc("tools/list", None, 1);
    let responses = mcp_call(&[&list]);

    assert_eq!(responses.len(), 1);
    let resp = &responses[0];
    assert!(resp["error"].is_object());
    assert_eq!(resp["error"]["code"], -32600); // Invalid request
}

#[test]
fn test_mcp_ping() {
    let ping = json_rpc("ping", None, 1);
    let responses = mcp_call(&[&ping]);

    assert_eq!(responses.len(), 1);
    let resp = &responses[0];
    assert!(resp["result"].is_object());
}

// ============================================================================
// Tool call tests (require temp directories)
// ============================================================================

#[test]
fn test_mcp_init_project() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {
                "name": "test_mcp_project",
                "cwd": cwd,
                "no_git": true
            }
        })),
        2,
    );
    let responses = mcp_call(&[&init, &call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    assert!(resp["result"].is_object());

    // Parse the tool result content
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["status"], "success");
    assert_eq!(result["project_name"], "test_mcp_project");
    assert!(!result["files_created"].as_array().unwrap().is_empty());

    // Verify files actually exist on disk
    let project_dir = tmpdir.path().join("test_mcp_project");
    assert!(project_dir.exists());
    assert!(project_dir.join("Cargo.toml").exists());
    assert!(project_dir.join("src/main.rs").exists());
}

#[test]
fn test_mcp_add_entity() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();

    // First create a project
    let init_call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {"name": "entity_test", "cwd": cwd, "no_git": true}
        })),
        2,
    );

    // Then add an entity (cwd must be inside the project)
    let project_cwd = tmpdir
        .path()
        .join("entity_test")
        .to_string_lossy()
        .to_string();
    let entity_call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "add_entity",
            "arguments": {
                "name": "product",
                "fields": "sku:String,price:f64",
                "cwd": project_cwd
            }
        })),
        3,
    );
    let responses = mcp_call(&[&init, &init_call, &entity_call]);

    assert_eq!(responses.len(), 3);
    let resp = &responses[2];
    assert!(resp["result"].is_object());
    assert!(resp["result"]["isError"].is_null() || resp["result"]["isError"] == Value::Null);

    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["status"], "success");
    assert_eq!(result["entity_name"], "product");

    // Verify entity files exist
    let entity_dir = tmpdir.path().join("entity_test/src/entities/product");
    assert!(entity_dir.exists());
    assert!(entity_dir.join("model.rs").exists());
}

#[test]
fn test_mcp_add_link() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();
    let init = initialize_msg();

    // Create project + 2 entities
    let init_call = json_rpc(
        "tools/call",
        Some(
            json!({"name": "init_project", "arguments": {"name": "link_test", "cwd": cwd, "no_git": true}}),
        ),
        2,
    );
    let project_cwd = tmpdir
        .path()
        .join("link_test")
        .to_string_lossy()
        .to_string();
    let entity1 = json_rpc(
        "tools/call",
        Some(json!({"name": "add_entity", "arguments": {"name": "order", "cwd": &project_cwd}})),
        3,
    );
    let entity2 = json_rpc(
        "tools/call",
        Some(json!({"name": "add_entity", "arguments": {"name": "invoice", "cwd": &project_cwd}})),
        4,
    );
    let link_call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "add_link",
            "arguments": {"source": "order", "target": "invoice", "cwd": &project_cwd}
        })),
        5,
    );
    let responses = mcp_call(&[&init, &init_call, &entity1, &entity2, &link_call]);

    assert_eq!(responses.len(), 5);
    let resp = &responses[4];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["status"], "success");
    assert!(result["link"].as_str().unwrap().contains("order"));
    assert!(result["link"].as_str().unwrap().contains("invoice"));
}

#[test]
fn test_mcp_get_project_info() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();
    let init = initialize_msg();

    let init_call = json_rpc(
        "tools/call",
        Some(
            json!({"name": "init_project", "arguments": {"name": "info_test", "cwd": cwd, "no_git": true}}),
        ),
        2,
    );
    let project_cwd = tmpdir
        .path()
        .join("info_test")
        .to_string_lossy()
        .to_string();
    let entity_call = json_rpc(
        "tools/call",
        Some(json!({"name": "add_entity", "arguments": {"name": "product", "cwd": &project_cwd}})),
        3,
    );
    let info_call = json_rpc(
        "tools/call",
        Some(json!({"name": "get_project_info", "arguments": {"cwd": &project_cwd}})),
        4,
    );
    let responses = mcp_call(&[&init, &init_call, &entity_call, &info_call]);

    assert_eq!(responses.len(), 4);
    let resp = &responses[3];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["project_name"], "info_test");
    assert!(!result["entities"].as_array().unwrap().is_empty());
    assert!(result["this_version"].is_string());
}

#[test]
fn test_mcp_check_project_health() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();
    let init = initialize_msg();

    let init_call = json_rpc(
        "tools/call",
        Some(
            json!({"name": "init_project", "arguments": {"name": "health_test", "cwd": cwd, "no_git": true}}),
        ),
        2,
    );
    let project_cwd = tmpdir
        .path()
        .join("health_test")
        .to_string_lossy()
        .to_string();
    let doctor_call = json_rpc(
        "tools/call",
        Some(json!({"name": "check_project_health", "arguments": {"cwd": &project_cwd}})),
        3,
    );
    let responses = mcp_call(&[&init, &init_call, &doctor_call]);

    assert_eq!(responses.len(), 3);
    let resp = &responses[2];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert!(result["diagnostics"].as_array().is_some());
    assert!(result["summary"]["total"].as_u64().unwrap() > 0);
    assert!(result["summary"]["pass"].is_number());
}

#[test]
fn test_mcp_unknown_tool_error() {
    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({"name": "nonexistent_tool", "arguments": {}})),
        2,
    );
    let responses = mcp_call(&[&init, &call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(content.contains("Unknown tool"));
    assert_eq!(resp["result"]["isError"], true);
}
