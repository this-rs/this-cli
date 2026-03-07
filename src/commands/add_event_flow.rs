use anyhow::{Context, Result, bail};
use colored::Colorize;

use super::AddEventFlowArgs;
use crate::utils::file_writer::FileWriter;
use crate::utils::{output, project};

/// Represents the events.yaml config structure
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EventsConfig {
    #[serde(default)]
    pub event_sinks: Vec<EventSink>,
    #[serde(default)]
    pub event_flows: Vec<EventFlow>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventSink {
    pub name: String,
    #[serde(rename = "type")]
    pub sink_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventFlow {
    pub name: String,
    pub trigger: String,
    pub steps: Vec<FlowStep>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FlowStep {
    #[serde(rename = "type")]
    pub step_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sink: Option<String>,
}

pub fn run(args: AddEventFlowArgs, writer: &dyn FileWriter) -> Result<()> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?;
    run_in(args, writer, &cwd)
}

/// Run the add event-flow command with an explicit starting directory.
pub(crate) fn run_in(
    args: AddEventFlowArgs,
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

    if writer.is_dry_run() {
        println!("🔍 {}", "Dry run — no files will be written".cyan().bold());
        println!();
    }

    output::print_step(&format!(
        "Adding event flow '{}' to config/events.yaml...",
        &args.name
    ));

    // Read and parse existing config
    let yaml_content = std::fs::read_to_string(&events_path)
        .with_context(|| format!("Failed to read: {}", events_path.display()))?;
    let mut config: EventsConfig =
        serde_yaml::from_str(&yaml_content).with_context(|| "Failed to parse events.yaml")?;

    // Check for duplicate flow
    if config.event_flows.iter().any(|f| f.name == args.name) {
        bail!("Event flow '{}' already exists in events.yaml", args.name);
    }

    // Validate that the target sink exists
    if !config.event_sinks.iter().any(|s| s.name == args.sink) {
        bail!(
            "Sink '{}' not found in events.yaml. Add it first with: this add sink {} --sink-type <type>",
            args.sink,
            args.sink
        );
    }

    // Create flow with a deliver step
    let flow = EventFlow {
        name: args.name.clone(),
        trigger: args.trigger.clone(),
        steps: vec![FlowStep {
            step_type: "deliver".to_string(),
            condition: None,
            sink: Some(args.sink.clone()),
        }],
    };

    config.event_flows.push(flow);

    // Write back
    let new_yaml =
        serde_yaml::to_string(&config).with_context(|| "Failed to serialize events.yaml")?;
    writer.update_file(&events_path, &yaml_content, &new_yaml)?;

    output::print_info(&format!("Flow name: {}", &args.name));
    output::print_info(&format!("Trigger: {}", &args.trigger));
    output::print_info(&format!("Deliver to: {}", &args.sink));

    output::print_success("Event flow added to config/events.yaml!");

    output::print_next_steps(&[
        "Available trigger patterns:",
        "  entity.created.*          - Any entity created",
        "  entity.updated.<type>     - Specific entity updated",
        "  entity.deleted.<type>     - Specific entity deleted",
        "Available flow operators: filter, map, batch, deduplicate, rate_limit, fan_out, resolve, deliver",
    ]);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AddEventFlowArgs;
    use tempfile::TempDir;

    /// Set up a minimal this-rs project with events.yaml
    fn setup_events_project(tmp: &TempDir) -> std::path::PathBuf {
        let project = tmp.path().join("evtest");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(project.join("config")).unwrap();

        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"evtest\"\n\n[dependencies]\nthis = \"0.1\"\n",
        )
        .unwrap();

        std::fs::write(
            project.join("config/events.yaml"),
            "event_sinks:\n  - name: in-app\n    type: in_app\nevent_flows: []\n",
        )
        .unwrap();

        project
    }

    fn make_args(name: &str) -> AddEventFlowArgs {
        AddEventFlowArgs {
            name: name.to_string(),
            trigger: "entity.created.*".to_string(),
            sink: "in-app".to_string(),
        }
    }

    fn read_events_config(project: &std::path::Path) -> EventsConfig {
        let content = std::fs::read_to_string(project.join("config/events.yaml")).unwrap();
        serde_yaml::from_str(&content).unwrap()
    }

    #[test]
    fn test_add_event_flow_basic() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = make_args("notify-on-create");
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_events_config(&project);
        assert_eq!(config.event_flows.len(), 1);
        assert_eq!(config.event_flows[0].name, "notify-on-create");
        assert_eq!(config.event_flows[0].trigger, "entity.created.*");
        assert_eq!(config.event_flows[0].steps.len(), 1);
        assert_eq!(config.event_flows[0].steps[0].step_type, "deliver");
        assert_eq!(
            config.event_flows[0].steps[0].sink.as_deref(),
            Some("in-app")
        );
    }

    #[test]
    fn test_add_event_flow_duplicate_errors() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        run_in(make_args("my-flow"), &writer, &project).unwrap();

        let result = run_in(make_args("my-flow"), &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("already exists"),
            "Error should mention duplicate: {}",
            err
        );
    }

    #[test]
    fn test_add_event_flow_missing_sink_errors() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddEventFlowArgs {
            name: "bad-flow".to_string(),
            trigger: "entity.created.*".to_string(),
            sink: "nonexistent".to_string(),
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "Error should mention missing sink: {}",
            err
        );
    }

    #[test]
    fn test_add_event_flow_missing_events_yaml_errors() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("noevents");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"noevents\"\n\n[dependencies]\nthis = \"0.1\"\n",
        )
        .unwrap();

        let writer = crate::mcp::handlers::McpFileWriter::new();
        let result = run_in(make_args("test"), &writer, &project);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("config/events.yaml not found"),
            "Error should mention missing events.yaml: {}",
            err
        );
    }

    #[test]
    fn test_add_event_flow_custom_trigger() {
        let tmp = TempDir::new().unwrap();
        let project = setup_events_project(&tmp);
        let writer = crate::mcp::handlers::McpFileWriter::new();

        let args = AddEventFlowArgs {
            name: "order-updated".to_string(),
            trigger: "entity.updated.order".to_string(),
            sink: "in-app".to_string(),
        };
        let result = run_in(args, &writer, &project);
        assert!(result.is_ok(), "Should succeed: {:?}", result.err());

        let config = read_events_config(&project);
        assert_eq!(config.event_flows[0].trigger, "entity.updated.order");
    }
}
