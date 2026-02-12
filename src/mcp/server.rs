//! MCP Server implementation
//!
//! Implements the MCP server that communicates over stdio using JSON-RPC 2.0.
//! Synchronous implementation — no tokio required.

use super::handlers::ToolHandler;
use super::protocol::*;
use super::tools::all_tools;
use anyhow::Result;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "this-cli";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP Server that handles JSON-RPC 2.0 requests over stdio
pub struct McpServer {
    tool_handler: ToolHandler,
    initialized: bool,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new() -> Self {
        Self {
            tool_handler: ToolHandler::new(),
            initialized: false,
        }
    }

    /// Run the server, reading from stdin and writing to stdout
    pub fn run(&mut self) -> Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let reader = BufReader::new(stdin.lock());
        let mut writer = stdout.lock();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            if line.is_empty() {
                continue;
            }

            let response = self.handle_message(&line);

            if let Some(resp) = response {
                let json = serde_json::to_string(&resp)?;
                writeln!(writer, "{}", json)?;
                writer.flush()?;
            }
        }

        Ok(())
    }

    /// Handle a single JSON-RPC message
    fn handle_message(&mut self, message: &str) -> Option<JsonRpcResponse> {
        // Parse the request
        let request: JsonRpcRequest = match serde_json::from_str(message) {
            Ok(r) => r,
            Err(e) => {
                return Some(JsonRpcResponse::error(
                    Value::Null,
                    JsonRpcError::parse_error(e.to_string()),
                ));
            }
        };

        // Get the request ID (notifications have no ID)
        let id = match &request.id {
            Some(id) => id.clone(),
            None => {
                // This is a notification, handle but don't respond
                self.handle_notification(&request);
                return None;
            }
        };

        // Handle the method
        let result = self.handle_request(&request);

        Some(match result {
            Ok(value) => JsonRpcResponse::success(id, value),
            Err(error) => JsonRpcResponse::error(id, error),
        })
    }

    /// Handle a notification (no response expected)
    fn handle_notification(&mut self, request: &JsonRpcRequest) {
        match request.method.as_str() {
            "notifications/initialized" => {
                // Client confirmed initialization — nothing to do
            }
            "notifications/cancelled" => {
                // Request cancelled — nothing to do in synchronous mode
            }
            _ => {
                // Unknown notification — ignore
            }
        }
    }

    /// Handle a request and return the result or error
    fn handle_request(&mut self, request: &JsonRpcRequest) -> Result<Value, JsonRpcError> {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(&request.params),
            "ping" => Ok(json!({})),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(&request.params),
            _ => Err(JsonRpcError::method_not_found(&request.method)),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&mut self, params: &Option<Value>) -> Result<Value, JsonRpcError> {
        let _params: InitializeParams = params
            .as_ref()
            .map(|p| serde_json::from_value(p.clone()))
            .transpose()
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?
            .unwrap_or(InitializeParams {
                protocol_version: PROTOCOL_VERSION.to_string(),
                capabilities: ClientCapabilities::default(),
                client_info: None,
            });

        self.initialized = true;

        let result = InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability {
                    list_changed: false,
                },
            },
            server_info: ServerInfo {
                name: SERVER_NAME.to_string(),
                version: SERVER_VERSION.to_string(),
            },
        };

        serde_json::to_value(result).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    /// Handle tools/list request
    fn handle_tools_list(&self) -> Result<Value, JsonRpcError> {
        if !self.initialized {
            return Err(JsonRpcError::invalid_request("Server not initialized"));
        }

        let tools = all_tools();
        let result = ToolsListResult { tools };

        serde_json::to_value(result).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }

    /// Handle tools/call request
    fn handle_tools_call(&self, params: &Option<Value>) -> Result<Value, JsonRpcError> {
        if !self.initialized {
            return Err(JsonRpcError::invalid_request("Server not initialized"));
        }

        let params: ToolCallParams = params
            .as_ref()
            .ok_or_else(|| JsonRpcError::invalid_params("params required"))?
            .clone()
            .pipe(serde_json::from_value)
            .map_err(|e| JsonRpcError::invalid_params(e.to_string()))?;

        let result = self.tool_handler.handle(&params.name, params.arguments);

        let tool_result = match result {
            Ok(value) => {
                ToolCallResult::success(serde_json::to_string_pretty(&value).unwrap_or_default())
            }
            Err(e) => ToolCallResult::error(format!("{:#}", e)),
        };

        serde_json::to_value(tool_result).map_err(|e| JsonRpcError::internal_error(e.to_string()))
    }
}

/// Extension trait for pipe operator
trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_initialize_request() {
        let request = r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"claude-code","version":"1.0"}},"id":1}"#;
        let req: JsonRpcRequest = serde_json::from_str(request).unwrap();
        assert_eq!(req.method, "initialize");
        assert!(req.params.is_some());
    }

    #[test]
    fn test_parse_tools_list_request() {
        let request = r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#;
        let req: JsonRpcRequest = serde_json::from_str(request).unwrap();
        assert_eq!(req.method, "tools/list");
    }

    #[test]
    fn test_parse_tools_call_request() {
        let request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"init_project","arguments":{"name":"test"}},"id":3}"#;
        let req: JsonRpcRequest = serde_json::from_str(request).unwrap();
        assert_eq!(req.method, "tools/call");

        let params: ToolCallParams = serde_json::from_value(req.params.unwrap()).unwrap();
        assert_eq!(params.name, "init_project");
    }

    #[test]
    fn test_handle_message_parse_error() {
        let mut server = McpServer::new();
        let resp = server.handle_message("not json at all");
        let resp = resp.unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, PARSE_ERROR);
    }

    #[test]
    fn test_handle_message_method_not_found() {
        let mut server = McpServer::new();
        // First initialize
        server.handle_message(r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}},"id":1}"#);
        // Then call unknown method
        let resp = server.handle_message(r#"{"jsonrpc":"2.0","method":"unknown/method","id":2}"#);
        let resp = resp.unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn test_handle_initialize() {
        let mut server = McpServer::new();
        let resp = server.handle_message(
            r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}},"id":1}"#,
        );
        let resp = resp.unwrap();
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "this-cli");
        assert!(server.initialized);
    }

    #[test]
    fn test_handle_tools_list_before_init() {
        let mut server = McpServer::new();
        let resp = server.handle_message(r#"{"jsonrpc":"2.0","method":"tools/list","id":1}"#);
        let resp = resp.unwrap();
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, INVALID_REQUEST);
    }

    #[test]
    fn test_handle_tools_list() {
        let mut server = McpServer::new();
        // Initialize first
        server.handle_message(r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}},"id":1}"#);
        // Then list tools
        let resp = server.handle_message(r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#);
        let resp = resp.unwrap();
        assert!(resp.result.is_some());
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 9);
    }

    #[test]
    fn test_handle_notification_no_response() {
        let mut server = McpServer::new();
        // Notification has no id field
        let resp =
            server.handle_message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        assert!(resp.is_none());
    }

    #[test]
    fn test_handle_ping() {
        let mut server = McpServer::new();
        let resp = server.handle_message(r#"{"jsonrpc":"2.0","method":"ping","id":1}"#);
        let resp = resp.unwrap();
        assert!(resp.result.is_some());
    }
}
