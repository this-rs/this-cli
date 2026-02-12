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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_tools_count() {
        assert_eq!(all_tools().len(), 5);
    }

    #[test]
    fn test_all_tools_have_non_empty_description() {
        for tool in all_tools() {
            assert!(!tool.description.is_empty(), "Tool {} has empty description", tool.name);
        }
    }

    #[test]
    fn test_all_tools_have_valid_schema() {
        for tool in all_tools() {
            assert_eq!(tool.input_schema.schema_type, "object", "Tool {} schema is not object", tool.name);
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
            let props = tool.input_schema.properties.as_ref()
                .expect(&format!("Tool {} has no properties", tool.name));
            assert!(
                props.get("cwd").is_some(),
                "Tool {} is missing 'cwd' parameter",
                tool.name
            );
        }
    }
}
