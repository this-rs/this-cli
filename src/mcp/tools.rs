//! MCP Tool definitions for this-cli commands

use super::protocol::{InputSchema, ToolDefinition};
use serde_json::json;

/// Return all available MCP tools
pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        init_project_tool(),
        add_entity_tool(),
        add_link_tool(),
        get_project_info_tool(),
        check_project_health_tool(),
        build_project_tool(),
        start_dev_tool(),
        add_target_tool(),
        generate_client_tool(),
    ]
}

fn init_project_tool() -> ToolDefinition {
    ToolDefinition {
        name: "init_project".to_string(),
        description: "Create a new this-rs project with Axum server, SurrealDB store, and entity system. Generates a complete Rust project with Cargo.toml, main.rs, router, and infrastructure modules.".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "name": {
                    "type": "string",
                    "description": "Project name (snake_case recommended, e.g. 'my_api')"
                },
                "path": {
                    "type": "string",
                    "description": "Parent directory where the project will be created (default: current directory)"
                },
                "no_git": {
                    "type": "boolean",
                    "description": "If true, do not initialize a git repository (default: false)"
                },
                "port": {
                    "type": "integer",
                    "description": "Default server port (default: 3000)"
                },
                "workspace": {
                    "type": "boolean",
                    "description": "If true, create a workspace layout with this.yaml and api/ subdirectory for multi-target projects (default: false)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory for the command. If provided, the command runs as if invoked from this directory."
                }
            })),
            required: Some(vec!["name".to_string()]),
        },
    }
}

fn add_entity_tool() -> ToolDefinition {
    ToolDefinition {
        name: "add_entity".to_string(),
        description: "Add a new entity (data model) to an existing this-rs project. Creates model, store, descriptor, and handler files. Registers the entity in the module system and router.".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "name": {
                    "type": "string",
                    "description": "Entity name (singular, snake_case, e.g. 'product')"
                },
                "fields": {
                    "type": "string",
                    "description": "Entity fields as 'field:Type' pairs, comma-separated. Example: 'sku:String,price:f64,description:Option<String>'"
                },
                "validated": {
                    "type": "boolean",
                    "description": "If true, use impl_data_entity_validated! macro with basic validators (default: false)"
                },
                "indexed": {
                    "type": "string",
                    "description": "Fields to index, comma-separated (default: 'name')"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs project root)"
                }
            })),
            required: Some(vec!["name".to_string()]),
        },
    }
}

fn add_link_tool() -> ToolDefinition {
    ToolDefinition {
        name: "add_link".to_string(),
        description: "Add a typed link between two entity types. Updates links.yaml configuration and optionally adds validation rules. Both entities must already exist in the project.".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "source": {
                    "type": "string",
                    "description": "Source entity type (e.g. 'order')"
                },
                "target": {
                    "type": "string",
                    "description": "Target entity type (e.g. 'invoice')"
                },
                "link_type": {
                    "type": "string",
                    "description": "Custom link type name (default: 'has_<target>')"
                },
                "forward": {
                    "type": "string",
                    "description": "Forward route name (default: pluralized target)"
                },
                "reverse": {
                    "type": "string",
                    "description": "Reverse route name (default: source)"
                },
                "description": {
                    "type": "string",
                    "description": "Human-readable link description"
                },
                "no_validation_rule": {
                    "type": "boolean",
                    "description": "If true, do not add a validation rule for this link (default: false)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs project root)"
                }
            })),
            required: Some(vec!["source".to_string(), "target".to_string()]),
        },
    }
}

fn get_project_info_tool() -> ToolDefinition {
    ToolDefinition {
        name: "get_project_info".to_string(),
        description: "Get detailed information about the current this-rs project: entities with their fields, links between entities, this-rs version, and coherence status.".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs project root)"
                }
            })),
            required: None,
        },
    }
}

fn check_project_health_tool() -> ToolDefinition {
    ToolDefinition {
        name: "check_project_health".to_string(),
        description: "Run diagnostics on the this-rs project: check Cargo.toml validity, entity file presence, module registration, store implementations, and links.yaml consistency. Returns structured results with pass/warn/error levels.".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs project root)"
                }
            })),
            required: None,
        },
    }
}

fn build_project_tool() -> ToolDefinition {
    ToolDefinition {
        name: "build_project".to_string(),
        description: "Build the project: compile API (cargo build), frontend (npm run build), or produce a single embedded binary. Can also generate an optimized Dockerfile. Requires a workspace (this.yaml).".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "embed": {
                    "type": "boolean",
                    "description": "Build a single binary with frontend embedded (npm build → copy dist → cargo build --features embedded-frontend)"
                },
                "api_only": {
                    "type": "boolean",
                    "description": "Only build the API (cargo build)"
                },
                "front_only": {
                    "type": "boolean",
                    "description": "Only build the frontend (npm run build)"
                },
                "docker": {
                    "type": "boolean",
                    "description": "Generate an optimized multi-stage Dockerfile"
                },
                "release": {
                    "type": "boolean",
                    "description": "Build in release mode (default: true)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs workspace)"
                }
            })),
            required: None,
        },
    }
}

fn start_dev_tool() -> ToolDefinition {
    ToolDefinition {
        name: "start_dev".to_string(),
        description: "Start development servers: API (cargo run with auto-detected watcher) + frontend (npm run dev). Returns the command to run rather than spawning long-lived processes. Requires a workspace (this.yaml).".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "api_only": {
                    "type": "boolean",
                    "description": "Only start the API server (skip frontend)"
                },
                "no_watch": {
                    "type": "boolean",
                    "description": "Disable auto-detection of cargo-watch, force plain cargo run"
                },
                "port": {
                    "type": "integer",
                    "description": "Override the API port from this.yaml"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs workspace)"
                }
            })),
            required: None,
        },
    }
}

fn add_target_tool() -> ToolDefinition {
    ToolDefinition {
        name: "add_target".to_string(),
        description: "Add a deployment target to the workspace: webapp (React/Vue/Svelte SPA), desktop (Tauri), or mobile (iOS/Android via Capacitor). Updates this.yaml and scaffolds the target directory with framework boilerplate.".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "target_type": {
                    "type": "string",
                    "enum": ["webapp", "desktop", "ios", "android"],
                    "description": "Type of target to add"
                },
                "framework": {
                    "type": "string",
                    "description": "Frontend framework for webapp target (react, vue, svelte). Default: react"
                },
                "name": {
                    "type": "string",
                    "description": "Custom name for the target directory (default: auto-generated from type, e.g. 'front' for webapp)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs workspace)"
                }
            })),
            required: Some(vec!["target_type".to_string()]),
        },
    }
}

fn generate_client_tool() -> ToolDefinition {
    ToolDefinition {
        name: "generate_client".to_string(),
        description: "Generate a typed API client from the project's entities and links. Introspects model.rs files, descriptors, and links.yaml to produce TypeScript interfaces and CRUD functions. Requires entities to exist in the project.".to_string(),
        input_schema: InputSchema {
            schema_type: "object".to_string(),
            properties: Some(json!({
                "lang": {
                    "type": "string",
                    "enum": ["typescript"],
                    "description": "Target language for the generated client (default: typescript)"
                },
                "output": {
                    "type": "string",
                    "description": "Output file path. Default: auto-detected from this.yaml webapp target (e.g. front/src/api-client.ts)"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory (must be inside a this-rs project)"
                }
            })),
            required: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_tools_count() {
        assert_eq!(all_tools().len(), 9);
    }

    #[test]
    fn test_all_tools_have_non_empty_description() {
        for tool in all_tools() {
            assert!(
                !tool.description.is_empty(),
                "Tool {} has empty description",
                tool.name
            );
        }
    }

    #[test]
    fn test_all_tools_have_valid_schema() {
        for tool in all_tools() {
            assert_eq!(
                tool.input_schema.schema_type, "object",
                "Tool {} schema is not object",
                tool.name
            );
        }
    }

    #[test]
    fn test_init_project_required_fields() {
        let tool = init_project_tool();
        let required = tool.input_schema.required.unwrap();
        assert_eq!(required, vec!["name"]);
    }

    #[test]
    fn test_add_link_required_fields() {
        let tool = add_link_tool();
        let required = tool.input_schema.required.unwrap();
        assert_eq!(required, vec!["source", "target"]);
    }

    #[test]
    fn test_all_tools_have_cwd_param() {
        for tool in all_tools() {
            let props = tool
                .input_schema
                .properties
                .as_ref()
                .unwrap_or_else(|| panic!("Tool {} has no properties", tool.name));
            assert!(
                props.get("cwd").is_some(),
                "Tool {} is missing 'cwd' parameter",
                tool.name
            );
        }
    }
}
