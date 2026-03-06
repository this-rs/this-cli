use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::AddSinkArgs;
use crate::commands::add_event_flow::EventsConfig;
use crate::utils::file_writer::FileWriter;
use crate::utils::{output, project};

const VALID_SINK_TYPES: &[&str] = &["in_app", "webhook", "push", "websocket", "counter"];

pub fn run(args: AddSinkArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
    run_in(args, writer, &cwd)
}

/// Run the add sink command with an explicit starting directory.
pub(crate) fn run_in(
    args: AddSinkArgs,
    writer: &dyn FileWriter,
    cwd: &std::path::Path,
) -> Result<()> {
    let project_root = project::detect_project_root_from(cwd)?;
    let events_path = project_root.join("config/events.yaml");

    if !events_path.exists() {
        bail!(
            "config/events.yaml not found at {}. Run 'this init --events' first or create it manually.",
            events_path.display()
        );
    }

    // Validate sink type
    if !VALID_SINK_TYPES.contains(&args.sink_type.as_str()) {
        bail!(
            "Invalid sink type '{}'. Valid types: {}",
            args.sink_type,
            VALID_SINK_TYPES.join(", ")
        );
    }

    // Webhook requires URL
    if args.sink_type == "webhook" && args.url.is_none() {
        bail!("Webhook sinks require --url. Example: this add sink my-hook --sink-type webhook --url https://example.com/webhook");
    }

    if writer.is_dry_run() {
        println!("🔍 {}", "Dry run — no files will be written".cyan().bold());
        println!();
    }

    output::print_step(&format!(
        "Adding sink '{}' ({}) to config/events.yaml...",
        &args.name, &args.sink_type
    ));

    // Read and parse existing config
    let yaml_content = std::fs::read_to_string(&events_path)
        .with_context(|| format!("Failed to read: {}", events_path.display()))?;
    let mut config: EventsConfig =
        serde_yaml::from_str(&yaml_content).with_context(|| "Failed to parse events.yaml")?;

    // Check for duplicate sink
    if config.event_sinks.iter().any(|s| s.name == args.name) {
        bail!(
            "Sink '{}' already exists in events.yaml",
            args.name
        );
    }

    // Add sink
    let sink = crate::commands::add_event_flow::EventSink {
        name: args.name.clone(),
        sink_type: args.sink_type.clone(),
        url: args.url.clone(),
    };

    config.event_sinks.push(sink);

    // Write back
    let new_yaml =
        serde_yaml::to_string(&config).with_context(|| "Failed to serialize events.yaml")?;
    writer.update_file(&events_path, &yaml_content, &new_yaml)?;

    output::print_info(&format!("Sink name: {}", &args.name));
    output::print_info(&format!("Sink type: {}", &args.sink_type));
    if let Some(ref url) = args.url {
        output::print_info(&format!("URL: {}", url));
    }

    output::print_success("Event sink added to config/events.yaml!");

    output::print_next_steps(&[
        "You can now use this sink in event flows:",
        &format!(
            "  this add event-flow my-flow --trigger \"entity.created.*\" --sink {}",
            &args.name
        ),
        "Available sink types: in_app, webhook, push, websocket, counter",
    ]);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AddSinkArgs;
    use crate::commands::add_event_flow::EventsConfig;
    use tempfile::TempDir;

    fn setup_events_project(tmp: &TempDir) -> std::path::PathBuf {
        let project = tmp.path().join("sinktest");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("config")).unwrap();

        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"sinktest\"\n\n[dependencies]\nthis = \"0.1\"\n",
        )
        .unwrap();

        std::fs::write(
            project.join("config/events.yaml"),
            "event_sinks:\n  - name: in-app\n    type: in_app\nevent_flows: []\n",
        )
        .unwrap();

        project
    }

    fn read_events_config(project: &std::path::Path) -> EventsConfig {
        let content = std::fs::read_to_string(project.join("config/events.yaml")).unwrap();
        serde_yaml::from_str(&content).unwrap()
    }

    #[test]
    fn test_add_sink_in_app() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddSinkArgs {
            name: "my-notifications".to_string(),
            sink_type: "in_app".to_string(),
            url: None,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_events_config(&project);
        assert_eq!(config.event_sinks.len(), 2);
        assert_eq!(config.event_sinks[1].name, "my-notifications");
        assert_eq!(config.event_sinks[1].sink_type, "in_app");
    }

    #[test]
    fn test_add_sink_webhook_with_url() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddSinkArgs {
            name: "my-webhook".to_string(),
            sink_type: "webhook".to_string(),
            url: Some("https://example.com/webhook".to_string()),
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_events_config(&project);
        assert_eq!(config.event_sinks[1].name, "my-webhook");
        assert_eq!(config.event_sinks[1].sink_type, "webhook");
        assert_eq!(
            config.event_sinks[1].url.as_deref(),
            Some("https://example.com/webhook")
        );
    }

    #[test]
    fn test_add_sink_webhook_without_url_errors() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddSinkArgs {
            name: "bad-hook".to_string(),
            sink_type: "webhook".to_string(),
            url: None,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("--url"),
            "Error should mention --url: {}",
            err
        );
    }

    #[test]
    fn test_add_sink_invalid_type_errors() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddSinkArgs {
            name: "bad".to_string(),
            sink_type: "invalid_type".to_string(),
            url: None,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid sink type"),
            "Error should mention invalid type: {}",
            err
        );
    }

    #[test]
    fn test_add_sink_duplicate_errors() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        // "in-app" already exists in the initial config
        let args = AddSinkArgs {
            name: "in-app".to_string(),
            sink_type: "in_app".to_string(),
            url: None,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention duplicate: {}",
            err
        );
    }

    #[test]
    fn test_add_sink_counter() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddSinkArgs {
            name: "metrics".to_string(),
            sink_type: "counter".to_string(),
            url: None,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_events_config(&project);
        assert_eq!(config.event_sinks[1].sink_type, "counter");
    }

    #[test]
    fn test_add_sink_websocket() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddSinkArgs {
            name: "ws-sink".to_string(),
            sink_type: "websocket".to_string(),
            url: None,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_events_config(&project);
        assert_eq!(config.event_sinks[1].sink_type, "websocket");
    }

    #[test]
    fn test_add_sink_missing_events_yaml_errors() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("nosinktest");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"nosinktest\"\n\n[dependencies]\nthis = \"0.1\"\n",
        )
        .unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let args = AddSinkArgs {
            name: "test".to_string(),
            sink_type: "in_app".to_string(),
            url: None,
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("config/events.yaml not found"),
            "Error should mention missing events.yaml: {}",
            err
        );
    }
}
