//! Integration tests for the MCP server (`this mcp`)
//!
//! These tests spawn `this mcp` as a child process, send JSON-RPC messages
//! on stdin, and verify the responses on stdout.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn this_bin() -> String {
    // Cargo builds the binary for integration tests and gives its path, whatever
    // CARGO_TARGET_DIR is (the target/debug guesses below miss a shared target dir).
    let built = std::path::Path::new(env!("CARGO_BIN_EXE_this"));
    if built.exists() {
        return built.to_string_lossy().into_owned();
    }
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

/// Everything one `this mcp` session produced.
#[derive(Debug)]
struct McpSession {
    /// JSON-RPC messages read from stdout, in order.
    responses: Vec<Value>,
    /// Diagnostics read from stderr (banners, "Next steps", warnings...).
    stderr: String,
}

/// Send JSON-RPC messages to `this mcp` and collect its whole output.
///
/// # Contract asserted here
///
/// In MCP mode **stdout carries the JSON-RPC protocol and nothing else**. Any
/// other line on stdout is a corrupted channel, so this helper panics on it
/// instead of skipping it: a client that parses stdout as a JSON-RPC stream
/// cannot ignore a stray line, so neither may we.
///
/// Measured before the fix (this-cli 0.0.5), one `tools/call init_project`:
/// 24 lines on stdout for 2 responses — 19 human-readable lines (banner,
/// `📄 <file>`, "Next steps") and an empty stderr. Reproduce with:
///
/// ```text
/// printf '%s\n' '<initialize>' '<tools/call init_project>' | this mcp
/// ```
///
/// Human output belongs on stderr, which is why the banner is asserted there
/// rather than being dropped (`Stdio::null()` used to hide it).
fn mcp_session(messages: &[&str]) -> McpSession {
    let mut child = Command::new(this_bin())
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn this mcp");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    for msg in messages {
        writeln!(stdin, "{}", msg).expect("Failed to write to stdin");
    }
    drop(stdin); // Close stdin to signal EOF

    // Read stderr on its own thread: the fix routes *all* human output there,
    // so a full pipe must not be able to block us while we read stdout.
    let stderr_pipe = child.stderr.take().expect("Failed to open stderr");
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr_pipe);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            buf.push_str(&line);
            line.clear();
        }
        buf
    });

    let stdout = child.stdout.take().expect("Failed to open stdout");
    let reader = BufReader::new(stdout);

    let mut responses = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line.expect("Failed to read a stdout line");
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(&line).unwrap_or_else(|e| {
            panic!(
                "stdout line {} is not valid JSON ({e}). \
                 In MCP mode stdout must carry the JSON-RPC protocol only — \
                 human output belongs on stderr.\nOffending line: {line}",
                index + 1
            )
        });
        assert_eq!(
            value["jsonrpc"],
            "2.0",
            "stdout line {} is JSON but not a JSON-RPC 2.0 message: {line}",
            index + 1
        );
        responses.push(value);
    }

    let stderr = stderr_reader.join().expect("stderr reader thread panicked");
    child.wait().expect("Failed to wait for child");

    McpSession { responses, stderr }
}

/// Send JSON-RPC messages to `this mcp` and collect all responses
///
/// Strict by construction: see [`mcp_session`].
fn mcp_call(messages: &[&str]) -> Vec<Value> {
    mcp_session(messages).responses
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
    assert_eq!(tools.len(), 17);

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"init_project"));
    assert!(tool_names.contains(&"add_entity"));
    assert!(tool_names.contains(&"add_link"));
    assert!(tool_names.contains(&"add_event_flow"));
    assert!(tool_names.contains(&"add_sink"));
    assert!(tool_names.contains(&"get_project_info"));
    assert!(tool_names.contains(&"check_project_health"));
    assert!(tool_names.contains(&"build_project"));
    assert!(tool_names.contains(&"start_dev"));
    assert!(tool_names.contains(&"dev_status"));
    assert!(tool_names.contains(&"dev_logs"));
    assert!(tool_names.contains(&"dev_stop"));
    assert!(tool_names.contains(&"add_target"));
    assert!(tool_names.contains(&"generate_client"));
    assert!(tool_names.contains(&"descriptor"));
    assert!(tool_names.contains(&"add_module"));

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

/// The MCP surface gives the same answers as the CLI: a rule outside the macro grammar is
/// refused without descriptor mode, accepted once `descriptor init` ran, and the
/// `descriptor` tool returns the materialized descriptor carrying it.
#[test]
fn test_mcp_descriptor_tool_round_trip() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();
    let project_cwd = tmpdir
        .path()
        .join("desc_test")
        .to_string_lossy()
        .to_string();
    let init = initialize_msg();
    let call = |id: u64, name: &str, arguments: Value| {
        json_rpc(
            "tools/call",
            Some(json!({"name": name, "arguments": arguments})),
            id,
        )
    };
    let fields = "email:String:required:email,age:Option<i32>:max_value(150)";
    let msgs = [
        call(
            2,
            "init_project",
            json!({"name": "desc_test", "cwd": cwd, "no_git": true}),
        ),
        call(
            3,
            "add_entity",
            json!({"name": "customer", "fields": fields, "cwd": &project_cwd}),
        ),
        call(
            4,
            "descriptor",
            json!({"action": "init", "cwd": &project_cwd}),
        ),
        call(
            5,
            "add_entity",
            json!({"name": "customer", "fields": fields, "cwd": &project_cwd}),
        ),
        call(
            6,
            "descriptor",
            json!({"action": "check", "cwd": &project_cwd}),
        ),
        call(
            7,
            "descriptor",
            json!({"action": "export", "cwd": &project_cwd}),
        ),
    ];
    let refs: Vec<&str> = std::iter::once(init.as_str())
        .chain(msgs.iter().map(String::as_str))
        .collect();
    let responses = mcp_call(&refs);
    assert_eq!(responses.len(), 7);
    let text = |i: usize| {
        responses[i]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // Without descriptor mode: refused (nothing would enforce `email`).
    assert_eq!(responses[2]["result"]["isError"], true, "{}", text(2));
    assert!(text(2).contains("this descriptor init"), "{}", text(2));
    assert!(responses[3]["result"]["isError"].is_null(), "{}", text(3));
    assert!(responses[4]["result"]["isError"].is_null(), "{}", text(4));

    let check: Value = serde_json::from_str(&text(5)).unwrap();
    assert_eq!(check["status"], "ok", "{check}");
    assert_eq!(check["report"]["errors"].as_array().unwrap().len(), 0);

    let export: Value = serde_json::from_str(&text(6)).unwrap();
    assert_eq!(export["status"], "success", "{export}");
    let customer = export["descriptor"]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "customer")
        .expect("customer in the descriptor")
        .clone();
    let email = customer["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "email")
        .expect("email field")
        .clone();
    assert!(email["validators"].to_string().contains("email"), "{email}");
    assert_eq!(email["proto_tag"], 16, "{email}");
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

// ============================================================================
// Workspace MCP tests
// ============================================================================

#[test]
fn test_mcp_init_project_workspace() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {
                "name": "ws_mcp_test",
                "cwd": cwd,
                "no_git": true,
                "workspace": true
            }
        })),
        2,
    );
    let responses = mcp_call(&[&init, &call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    assert!(resp["result"].is_object());

    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["status"], "success");
    assert_eq!(result["project_name"], "ws_mcp_test");

    // Verify workspace structure on disk
    let ws_dir = tmpdir.path().join("ws_mcp_test");
    assert!(ws_dir.exists(), "Workspace dir should exist");
    assert!(ws_dir.join("this.yaml").exists(), "this.yaml should exist");
    assert!(
        ws_dir.join("api/Cargo.toml").exists(),
        "api/Cargo.toml should exist"
    );
    assert!(
        ws_dir.join("api/src/main.rs").exists(),
        "api/src/main.rs should exist"
    );
    assert!(
        ws_dir.join("api/dist/.gitkeep").exists(),
        "api/dist/.gitkeep should exist"
    );
}

#[test]
fn test_mcp_get_project_info_workspace() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();
    let init = initialize_msg();

    // Create workspace project
    let init_call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {"name": "ws_info_test", "cwd": cwd, "no_git": true, "workspace": true}
        })),
        2,
    );

    // Get info from workspace root
    let ws_cwd = tmpdir
        .path()
        .join("ws_info_test")
        .to_string_lossy()
        .to_string();
    let info_call = json_rpc(
        "tools/call",
        Some(json!({"name": "get_project_info", "arguments": {"cwd": &ws_cwd}})),
        3,
    );
    let responses = mcp_call(&[&init, &init_call, &info_call]);

    assert_eq!(responses.len(), 3);
    let resp = &responses[2];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();

    // Should have workspace info in the response
    assert!(
        result["workspace"].is_object(),
        "Should include workspace info"
    );
    assert_eq!(result["workspace"]["name"], "ws_info_test");
    assert_eq!(result["workspace"]["api_path"], "api");
    assert_eq!(result["workspace"]["api_port"], 3000);
}

// ============================================================================
// Build & Dev MCP tests
// ============================================================================

#[test]
fn test_mcp_build_project_outside_workspace_error() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "build_project",
            "arguments": {"cwd": cwd}
        })),
        2,
    );
    let responses = mcp_call(&[&init, &call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        content.contains("Not a this-rs workspace"),
        "Should say not a workspace. content: {}",
        content
    );
    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn test_mcp_build_project_docker_generates_dockerfile() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();
    let init = initialize_msg();

    // Create workspace
    let init_call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {"name": "build_mcp", "cwd": cwd, "no_git": true, "workspace": true}
        })),
        2,
    );

    // Add webapp target to this.yaml manually
    let ws_dir = tmpdir.path().join("build_mcp");
    let responses_init = mcp_call(&[&init, &init_call]);
    assert_eq!(responses_init.len(), 2);

    let this_yaml = r#"name: build_mcp
api:
  path: api
  port: 3000
targets:
  - target_type: webapp
    framework: react
    path: front
"#;
    std::fs::write(ws_dir.join("this.yaml"), this_yaml).unwrap();

    // Build with --docker
    let ws_cwd = ws_dir.to_string_lossy().to_string();
    let build_call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "build_project",
            "arguments": {"docker": true, "cwd": ws_cwd}
        })),
        3,
    );
    let init2 = initialize_msg();
    let responses = mcp_call(&[&init2, &build_call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["status"], "success");
    assert_eq!(result["mode"], "docker");

    // Verify Dockerfile was created
    assert!(
        ws_dir.join("Dockerfile").exists(),
        "Dockerfile should exist"
    );
}

#[test]
fn test_mcp_start_dev_outside_workspace_error() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "start_dev",
            "arguments": {"cwd": cwd}
        })),
        2,
    );
    let responses = mcp_call(&[&init, &call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        content.contains("Not a this-rs workspace"),
        "Should say not a workspace. content: {}",
        content
    );
    assert_eq!(resp["result"]["isError"], true);
}

#[test]
fn test_mcp_init_project_websocket() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {
                "name": "ws_project",
                "cwd": cwd,
                "no_git": true,
                "websocket": true
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
    assert_eq!(result["project_name"], "ws_project");
    assert_eq!(result["websocket_enabled"], true);

    // Verify Cargo.toml contains websocket feature
    let project_dir = tmpdir.path().join("ws_project");
    let cargo_toml = std::fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains(r#"features = ["websocket"]"#),
        "Cargo.toml should contain websocket feature. Got:\n{}",
        cargo_toml
    );

    // Verify main.rs contains WebSocketExposure
    let main_rs = std::fs::read_to_string(project_dir.join("src/main.rs")).unwrap();
    assert!(
        main_rs.contains("WebSocketExposure"),
        "main.rs should contain WebSocketExposure. Got:\n{}",
        main_rs
    );
}

#[test]
fn test_mcp_init_project_grpc() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {
                "name": "grpc_project",
                "cwd": cwd,
                "no_git": true,
                "grpc": true
            }
        })),
        2,
    );
    let responses = mcp_call(&[&init, &call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    assert!(resp["result"].is_object());

    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["status"], "success");
    assert_eq!(result["project_name"], "grpc_project");
    assert_eq!(result["grpc_enabled"], true);

    // Verify Cargo.toml contains grpc feature
    let project_dir = tmpdir.path().join("grpc_project");
    let cargo_toml = std::fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains(r#"features = ["grpc"]"#),
        "Cargo.toml should contain grpc feature. Got:\n{}",
        cargo_toml
    );

    // Verify main.rs contains GrpcExposure
    let main_rs = std::fs::read_to_string(project_dir.join("src/main.rs")).unwrap();
    assert!(
        main_rs.contains("GrpcExposure"),
        "main.rs should contain GrpcExposure. Got:\n{}",
        main_rs
    );
}

#[test]
fn test_mcp_init_project_grpc_websocket() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let call = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {
                "name": "both_project",
                "cwd": cwd,
                "no_git": true,
                "grpc": true,
                "websocket": true
            }
        })),
        2,
    );
    let responses = mcp_call(&[&init, &call]);

    assert_eq!(responses.len(), 2);
    let resp = &responses[1];
    assert!(resp["result"].is_object());

    let content = resp["result"]["content"][0]["text"].as_str().unwrap();
    let result: Value = serde_json::from_str(content).unwrap();
    assert_eq!(result["status"], "success");
    assert_eq!(result["grpc_enabled"], true);
    assert_eq!(result["websocket_enabled"], true);

    // Verify Cargo.toml contains both features
    let project_dir = tmpdir.path().join("both_project");
    let cargo_toml = std::fs::read_to_string(project_dir.join("Cargo.toml")).unwrap();
    assert!(
        cargo_toml.contains(r#"features = ["grpc", "websocket"]"#),
        "Cargo.toml should contain both features. Got:\n{}",
        cargo_toml
    );

    // Verify main.rs contains both exposures
    let main_rs = std::fs::read_to_string(project_dir.join("src/main.rs")).unwrap();
    assert!(
        main_rs.contains("GrpcExposure"),
        "main.rs should contain GrpcExposure. Got:\n{}",
        main_rs
    );
    assert!(
        main_rs.contains("WebSocketExposure"),
        "main.rs should contain WebSocketExposure. Got:\n{}",
        main_rs
    );
}

// ============================================================================
// Channel purity — stdout carries the protocol, stderr carries the humans
// ============================================================================

/// The whole point: a client parses stdout as a JSON-RPC stream. Human output
/// (banner, `📄 file`, "Next steps") must not appear there.
///
/// This test failed before the fix, with 19 non-JSON lines on stdout for the
/// `init_project` call alone. It exercises several tools in ONE session on
/// purpose: every tool call that prints is a chance to corrupt the channel.
#[test]
fn test_mcp_stdout_is_protocol_only() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let init_project = json_rpc(
        "tools/call",
        Some(json!({
            "name": "init_project",
            "arguments": {
                "name": "purity_project",
                "cwd": cwd,
                "no_git": true
            }
        })),
        2,
    );
    let add_entity = json_rpc(
        "tools/call",
        Some(json!({
            "name": "add_entity",
            "arguments": {
                "name": "purity_project",
                "entity_name": "widget",
                "fields": "name:string,price:f64",
                "cwd": cwd
            }
        })),
        3,
    );
    let info = json_rpc(
        "tools/call",
        Some(json!({
            "name": "get_project_info",
            "arguments": { "cwd": cwd }
        })),
        4,
    );

    // mcp_session panics on any stdout line that is not JSON-RPC 2.0, so
    // reaching the assertions below already proves the channel is clean.
    let session = mcp_session(&[&init, &init_project, &add_entity, &info]);

    assert_eq!(
        session.responses.len(),
        4,
        "one response per request; got {:#?}",
        session.responses
    );

    // And the human output still exists — it moved, it did not disappear.
    assert!(
        session.stderr.contains("this-rs project builder"),
        "the CLI banner should be on stderr, not dropped. Got:\n{}",
        session.stderr
    );
    assert!(
        session.stderr.contains("created successfully"),
        "the success message should be on stderr. Got:\n{}",
        session.stderr
    );
}

/// `ping` is the quietest tool: proving purity on it isolates the protocol
/// itself from any command's printing.
#[test]
fn test_mcp_ping_stdout_is_protocol_only() {
    let init = initialize_msg();
    let ping = json_rpc("ping", None, 2);
    let session = mcp_session(&[&init, &ping]);

    assert_eq!(session.responses.len(), 2);
    assert_eq!(session.responses[1]["result"], json!({}));
}

// ============================================================================
// start_dev — supervised sessions, protocol never blocked
// ============================================================================

/// `start_dev` used to call `dev::run` synchronously: the tool's description
/// promised "returns the command", the handler froze the whole JSON-RPC loop.
///
/// Non-blocking is proven at the unit level
/// (`mcp::dev_session::tests::start_returns_while_the_child_keeps_running_and_stop_ends_it`,
/// which supervises a real 30-second child). What this test pins is the
/// protocol behaviour: `start_dev` answers, the transport keeps serving the next
/// request, and a failed start leaves no phantom session behind.
#[test]
fn test_mcp_start_dev_answers_and_leaves_the_transport_alive() {
    let tmpdir = tempfile::tempdir().unwrap();
    let cwd = tmpdir.path().to_string_lossy().to_string();

    let init = initialize_msg();
    let start = json_rpc(
        "tools/call",
        Some(json!({
            "name": "start_dev",
            "arguments": {"cwd": cwd}
        })),
        2,
    );
    let status = json_rpc(
        "tools/call",
        Some(json!({
            "name": "dev_status",
            "arguments": {}
        })),
        3,
    );
    let ping = json_rpc("ping", None, 4);

    let session = mcp_session(&[&init, &start, &status, &ping]);

    assert_eq!(
        session.responses.len(),
        4,
        "every request must be answered, including the ones after start_dev"
    );

    // Outside a workspace `this dev` dies at once: the tool must report that,
    // not hand back a session id for a process that is already gone.
    let started = &session.responses[1];
    assert_eq!(
        started["result"]["isError"], true,
        "start_dev outside a workspace should fail: {started}"
    );
    let text = started["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("Not a this-rs workspace"),
        "the tool should surface the child's own error, got: {text}"
    );

    // No session was registered, so the registry is empty and still answerable.
    let status_content = session.responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    let status_json: Value = serde_json::from_str(status_content).unwrap();
    assert_eq!(
        status_json["sessions"].as_array().unwrap().len(),
        0,
        "a failed start must not register a session: {status_json}"
    );

    assert_eq!(session.responses[3]["result"], json!({}));
}

/// `dev_logs` and `dev_stop` are the other half of the session surface: without
/// them `start_dev` would hand back a session nobody can inspect or close.
/// Exercised through the protocol, on an empty registry — the state a fresh
/// server is in.
#[test]
fn test_mcp_dev_logs_and_dev_stop_without_a_session() {
    let init = initialize_msg();
    let logs = json_rpc(
        "tools/call",
        Some(json!({"name": "dev_logs", "arguments": {}})),
        2,
    );
    let stop = json_rpc(
        "tools/call",
        Some(json!({"name": "dev_stop", "arguments": {}})),
        3,
    );

    let session = mcp_session(&[&init, &logs, &stop]);
    assert_eq!(session.responses.len(), 3);

    // Asking for logs when nothing ran is an error that says exactly that.
    assert_eq!(session.responses[1]["result"]["isError"], true);
    let text = session.responses[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(text.contains("No dev session is running"), "got: {text}");

    // Stopping nothing is a no-op, not an error: a stop that cannot fail is
    // what a client wants to be able to call unconditionally.
    let text = session.responses[2]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    let stopped: Value = serde_json::from_str(text).unwrap();
    assert_eq!(stopped["status"], "none");
    assert_eq!(stopped["stopped"].as_array().unwrap().len(), 0);
}
