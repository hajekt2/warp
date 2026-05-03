use std::{ffi::OsString, sync::Arc, time::Duration};

use futures::channel::oneshot;
use warp_acp::{McpServer, McpServerSse, McpServerStdio, SessionUpdate};
use warp_cli::agent::Harness;
use warp_cli::{
    OZ_CLI_ENV, OZ_HARNESS_ENV, OZ_PARENT_RUN_ID_ENV, OZ_RUN_ID_ENV, SERVER_ROOT_URL_OVERRIDE_ENV,
    SESSION_SHARING_SERVER_URL_OVERRIDE_ENV, WS_SERVER_URL_OVERRIDE_ENV,
};
use warp_core::channel::ChannelState;

use super::{
    AcpStreamingOutputBuilder, AgentDriver, IdleTimeoutSender,
    LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV, LEGACY_OZ_PARENT_STATE_ROOT_ENV,
    OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV, OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
};
use crate::ai::agent::{
    task::TaskId, AIAgentActionResult, AIAgentActionResultType, AIAgentInput, AIAgentOutput,
    AIAgentOutputMessage, ArtifactCreatedData, MessageId, UploadArtifactResult,
};
use crate::ai::mcp::parsing::normalize_mcp_json;
use crate::ai::{agent_sdk::task_env_vars, ambient_agents::AmbientAgentTaskId};

#[test]
fn test_normalize_single_cli_server() {
    let input = r#"{"command": "npx", "args": ["-y", "mcp-server"]}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should wrap with a generated name
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(server["command"].as_str().unwrap(), "npx");
}

#[test]
fn test_normalize_single_sse_server() {
    let input = r#"{"url": "http://localhost:3000/mcp", "headers": {"API_KEY": "value"}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should wrap with a generated name
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(server["url"].as_str().unwrap(), "http://localhost:3000/mcp");
}

#[test]
fn test_normalize_already_wrapped_server() {
    let input = r#"{"my-server": {"command": "npx", "args": []}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should return as-is (no command/url at top level)
    assert_eq!(result, input);
}

#[test]
fn test_normalize_mcp_servers_wrapper() {
    let input = r#"{"mcpServers": {"server-name": {"command": "npx", "args": []}}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should return as-is (no command/url at top level)
    assert_eq!(result, input);
}

#[test]
fn test_normalize_servers_wrapper() {
    let input = r#"{"servers": {"server-name": {"url": "http://example.com"}}}"#;
    let result = normalize_mcp_json(input).unwrap();

    // Should return as-is (no command/url at top level)
    assert_eq!(result, input);
}

#[test]
fn test_normalize_invalid_json() {
    let input = "not valid json";
    let result = normalize_mcp_json(input);

    assert!(result.is_err());
}

#[test]
fn test_normalize_cli_server_with_env() {
    let input = r#"{"command": "npx", "args": ["-y", "mcp-server"], "env": {"API_KEY": "secret"}}"#;
    let result = normalize_mcp_json(input).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(server["env"]["API_KEY"].as_str().unwrap(), "secret");
}

#[test]
fn test_normalize_sse_server_with_headers() {
    let input =
        r#"{"url": "http://localhost:5000/mcp", "headers": {"Authorization": "Bearer token"}}"#;
    let result = normalize_mcp_json(input).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    let parsed = parsed.as_object().unwrap();
    assert_eq!(parsed.len(), 1);
    let (_name, server) = parsed.iter().next().unwrap();
    assert_eq!(
        server["headers"]["Authorization"].as_str().unwrap(),
        "Bearer token"
    );
}

#[test]
fn acp_mcp_explicit_stdio_allowlist_entry_parses_as_argv() {
    let server = AgentDriver::parse_acp_mcp_server("local-tools | /bin/echo hello world")
        .expect("explicit ACP MCP allowlist entry should parse");

    let McpServer::Stdio(server) = server else {
        panic!("expected stdio MCP server");
    };
    assert_eq!(server.name, "local-tools");
    assert_eq!(server.command, std::path::PathBuf::from("/bin/echo"));
    assert_eq!(server.args, &["hello", "world"]);
}

#[test]
fn acp_mcp_bare_allowlist_entry_is_reserved_for_installed_servers() {
    assert!(AgentDriver::parse_acp_mcp_server("installed-server-name").is_none());
    assert!(AgentDriver::parse_acp_mcp_server("*").is_none());
}

#[test]
fn acp_mcp_capability_filter_gates_non_stdio_transports() {
    let servers = vec![
        McpServer::Stdio(McpServerStdio::new("stdio", "/bin/echo")),
        McpServer::Sse(McpServerSse::new("sse", "https://example.test/sse")),
    ];

    let filtered = AgentDriver::filter_acp_mcp_servers_for_agent_capabilities(
        servers.clone(),
        &serde_json::json!({"mcpCapabilities":{"sse": false}}),
    );
    assert_eq!(filtered.len(), 1);
    assert!(matches!(filtered[0], McpServer::Stdio(_)));

    let filtered = AgentDriver::filter_acp_mcp_servers_for_agent_capabilities(
        servers,
        &serde_json::json!({"mcpCapabilities":{"sse": true}}),
    );
    assert_eq!(filtered.len(), 2);
}

#[test]
fn acp_auth_method_prefers_method_id() {
    assert_eq!(
        AgentDriver::preferred_acp_auth_method(&[
            serde_json::json!({"methodId":"env-openai-api-key"}),
            serde_json::json!({"id":"fallback"}),
        ]),
        Some("env-openai-api-key".to_string())
    );
}

#[test]
fn acp_auth_method_skips_interactive_login_methods() {
    assert_eq!(
        AgentDriver::preferred_acp_auth_method(&[
            serde_json::json!({
                "id": "opencode-login",
                "name": "Login with opencode",
                "description": "Run `opencode auth login` in the terminal"
            }),
            serde_json::json!({"methodId":"env-openai-api-key"}),
        ]),
        Some("env-openai-api-key".to_string())
    );

    assert_eq!(
        AgentDriver::preferred_acp_auth_method(&[serde_json::json!({
            "id": "opencode-login",
            "name": "Login with opencode",
            "description": "Run `opencode auth login` in the terminal"
        })]),
        None
    );
}

#[test]
fn acp_streaming_output_builder_maps_structured_updates_to_warp_messages() {
    let mut builder = AcpStreamingOutputBuilder::default();

    assert!(builder.apply_update(SessionUpdate::AgentThoughtChunk {
        text: "thinking".to_string(),
    }));
    assert!(builder.apply_update(SessionUpdate::AgentMessageChunk {
        text: "hello".to_string(),
    }));
    assert!(builder.apply_update(SessionUpdate::ToolCall {
        id: "tool-1".to_string(),
        name: "read_file".to_string(),
        args: serde_json::json!({"path":"README.md"}),
    }));
    assert!(builder.apply_update(SessionUpdate::Plan {
        content: "1. Ship it".to_string(),
    }));
    assert!(!builder.apply_update(SessionUpdate::Unknown {
        method: "future".to_string(),
        params: serde_json::json!({"sessionUpdate":"future"}),
    }));

    let output = builder.output();

    assert!(output
        .text_from_agent_reasoning()
        .any(|text| agent_text_contains(text, "thinking")));
    assert!(output
        .text_from_agent_output()
        .any(|text| agent_text_contains(text, "hello")));
    assert!(output.actions().any(|action| {
        matches!(
            &action.action,
            crate::ai::agent::AIAgentActionType::CallMCPTool { name, .. } if name == "read_file"
        )
    }));
    assert!(output
        .text_from_agent_output()
        .any(|text| agent_text_contains(text, "Ship it")));
}

#[test]
fn acp_streaming_output_builder_replaces_replayed_tool_call_updates() {
    let mut builder = AcpStreamingOutputBuilder::default();

    assert!(builder.apply_update(SessionUpdate::ToolCallUpdate {
        id: "tool-1".to_string(),
        status: Some("running".to_string()),
        args: None,
        output: Some("first output".to_string()),
    }));
    assert!(builder.apply_update(SessionUpdate::ToolCallUpdate {
        id: "tool-1".to_string(),
        status: Some("completed".to_string()),
        args: None,
        output: Some("second output".to_string()),
    }));

    let output = builder.output();
    let update_messages = output
        .messages
        .iter()
        .filter(|message| message.id == MessageId::new("acp-tool-update-tool-1".to_string()))
        .collect::<Vec<_>>();

    assert_eq!(update_messages.len(), 1);
    let update_text = update_messages[0].to_string();
    assert!(update_text.contains("completed"));
    assert!(update_text.contains("second output"));
    assert!(!update_text.contains("first output"));
}

fn agent_text_contains(text: &crate::ai::agent::AIAgentText, needle: &str) -> bool {
    text.sections.iter().any(|section| match section {
        crate::ai::agent::AIAgentTextSection::PlainText { text } => text.text().contains(needle),
        crate::ai::agent::AIAgentTextSection::Code { code, .. } => code.contains(needle),
        crate::ai::agent::AIAgentTextSection::Table { table } => {
            table.markdown_source.contains(needle)
        }
        crate::ai::agent::AIAgentTextSection::Image { image } => {
            image.markdown_source.contains(needle)
        }
        crate::ai::agent::AIAgentTextSection::MermaidDiagram { diagram } => {
            diagram.markdown_source.contains(needle)
        }
    })
}

// ── IdleTimeoutSender tests ──────────────────────────────────────────────────────

#[test]
fn idle_timeout_sender_send_now_delivers_value() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_now(42);
    assert_eq!(rx.try_recv().unwrap(), Some(42));
}

#[test]
fn idle_timeout_sender_send_now_only_delivers_once() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_now(1);
    idle_timeout.end_run_now(2);
    assert_eq!(rx.try_recv().unwrap(), Some(1));
}

#[test]
fn idle_timeout_sender_send_after_delivers_after_timeout() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_after(Duration::from_millis(50), 99);

    // Not yet delivered.
    assert_eq!(rx.try_recv().unwrap(), None);

    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rx.try_recv().unwrap(), Some(99));
}

#[test]
fn idle_timeout_sender_cancel_prevents_delivery() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_after(Duration::from_millis(50), 99);
    idle_timeout.cancel_idle_timeout();

    std::thread::sleep(Duration::from_millis(100));
    // Sender was not consumed, so the channel is still open but empty.
    assert_eq!(rx.try_recv().unwrap(), None);
}

#[test]
fn idle_timeout_sender_cancel_then_send_now_delivers() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    idle_timeout.end_run_after(Duration::from_millis(50), 1);
    idle_timeout.cancel_idle_timeout();
    idle_timeout.end_run_now(2);

    assert_eq!(rx.try_recv().unwrap(), Some(2));
}

#[test]
fn idle_timeout_sender_later_send_after_supersedes_earlier() {
    let (tx, mut rx) = oneshot::channel::<i32>();
    let idle_timeout = IdleTimeoutSender::new(tx);
    // First timer: long timeout.
    idle_timeout.end_run_after(Duration::from_secs(10), 1);
    // Second timer: short timeout. The first is implicitly cancelled.
    idle_timeout.end_run_after(Duration::from_millis(50), 2);

    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(rx.try_recv().unwrap(), Some(2));
}

#[test]
fn task_env_vars_include_parent_run_id_when_present() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), Some("parent-run-123"), Harness::Claude);
    let overrides_allowed = ChannelState::channel().allows_server_url_overrides();

    assert_eq!(
        env_vars.get(&OsString::from(OZ_RUN_ID_ENV)),
        Some(&OsString::from(task_id.to_string()))
    );
    assert_eq!(
        env_vars.get(&OsString::from(OZ_PARENT_RUN_ID_ENV)),
        Some(&OsString::from("parent-run-123"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(OZ_HARNESS_ENV)),
        Some(&OsString::from("claude"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV)),
        Some(&OsString::from("1"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(
            LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV
        )),
        Some(&OsString::from("1"))
    );
    assert!(env_vars
        .get(&OsString::from(OZ_CLI_ENV))
        .is_some_and(|value| !value.is_empty()));

    let server_root_url = ChannelState::server_root_url().into_owned();
    if overrides_allowed && !server_root_url.is_empty() {
        assert_eq!(
            env_vars.get(&OsString::from(SERVER_ROOT_URL_OVERRIDE_ENV)),
            Some(&OsString::from(server_root_url))
        );
    } else {
        assert!(!env_vars.contains_key(&OsString::from(SERVER_ROOT_URL_OVERRIDE_ENV)));
    }

    let ws_server_url = ChannelState::ws_server_url().into_owned();
    if overrides_allowed && !ws_server_url.is_empty() {
        assert_eq!(
            env_vars.get(&OsString::from(WS_SERVER_URL_OVERRIDE_ENV)),
            Some(&OsString::from(ws_server_url))
        );
    } else {
        assert!(!env_vars.contains_key(&OsString::from(WS_SERVER_URL_OVERRIDE_ENV)));
    }

    if overrides_allowed {
        match ChannelState::session_sharing_server_url() {
            Some(url) if !url.is_empty() => assert_eq!(
                env_vars.get(&OsString::from(SESSION_SHARING_SERVER_URL_OVERRIDE_ENV)),
                Some(&OsString::from(url.into_owned()))
            ),
            _ => {
                assert!(!env_vars
                    .contains_key(&OsString::from(SESSION_SHARING_SERVER_URL_OVERRIDE_ENV)))
            }
        }
    } else {
        assert!(!env_vars.contains_key(&OsString::from(SESSION_SHARING_SERVER_URL_OVERRIDE_ENV)));
    }
}

#[test]
fn task_env_vars_omit_parent_run_id_when_absent() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440001".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), None, Harness::Oz);
    let overrides_allowed = ChannelState::channel().allows_server_url_overrides();

    assert_eq!(
        env_vars.get(&OsString::from(OZ_RUN_ID_ENV)),
        Some(&OsString::from(task_id.to_string()))
    );
    assert!(!env_vars.contains_key(&OsString::from(OZ_PARENT_RUN_ID_ENV)));
    assert_eq!(
        env_vars.get(&OsString::from(OZ_HARNESS_ENV)),
        Some(&OsString::from("oz"))
    );
    assert!(!env_vars.contains_key(&OsString::from(OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV)));
    assert!(!env_vars.contains_key(&OsString::from(
        LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV
    )));
    assert_eq!(
        env_vars.contains_key(&OsString::from(SERVER_ROOT_URL_OVERRIDE_ENV)),
        overrides_allowed && !ChannelState::server_root_url().is_empty()
    );
    assert_eq!(
        env_vars.contains_key(&OsString::from(WS_SERVER_URL_OVERRIDE_ENV)),
        overrides_allowed && !ChannelState::ws_server_url().is_empty()
    );
}

#[test]
fn task_env_vars_enable_external_parent_listener_for_claude_runs_without_parent_run_id() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440002".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), None, Harness::Claude);
    assert_eq!(
        env_vars.get(&OsString::from(OZ_MESSAGE_LISTENER_MANAGED_EXTERNALLY_ENV)),
        Some(&OsString::from("1"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(
            LEGACY_OZ_PARENT_LISTENER_MANAGED_EXTERNALLY_ENV
        )),
        Some(&OsString::from("1"))
    );
}

#[test]
#[serial_test::serial]
fn task_env_vars_propagate_message_listener_state_root_with_legacy_alias() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440003".parse().unwrap();
    std::env::set_var(
        OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
        "/tmp/message-listener-root",
    );
    let env_vars = task_env_vars(Some(&task_id), None, Harness::Claude);
    std::env::remove_var(OZ_MESSAGE_LISTENER_STATE_ROOT_ENV);

    assert_eq!(
        env_vars.get(&OsString::from(OZ_MESSAGE_LISTENER_STATE_ROOT_ENV)),
        Some(&OsString::from("/tmp/message-listener-root"))
    );
    assert_eq!(
        env_vars.get(&OsString::from(LEGACY_OZ_PARENT_STATE_ROOT_ENV)),
        Some(&OsString::from("/tmp/message-listener-root"))
    );
}

#[test]
fn task_env_vars_can_use_opencode_harness() {
    let task_id: AmbientAgentTaskId = "550e8400-e29b-41d4-a716-446655440004".parse().unwrap();
    let env_vars = task_env_vars(Some(&task_id), Some("parent-run-456"), Harness::OpenCode);

    assert_eq!(
        env_vars.get(&OsString::from(OZ_HARNESS_ENV)),
        Some(&OsString::from("opencode"))
    );
}

#[test]
fn json_format_output_includes_filename_for_file_artifact_created_event() {
    let output = AIAgentOutput {
        messages: vec![AIAgentOutputMessage::artifact_created(
            MessageId::new("message-1".to_string()),
            ArtifactCreatedData::File {
                artifact_uid: "artifact-uid".to_string(),
                filepath: "outputs/report.txt".to_string(),
                filename: "report.txt".to_string(),
                mime_type: "text/plain".to_string(),
                description: Some("Build output for the latest run".to_string()),
                size_bytes: 42,
            },
        )],
        ..Default::default()
    };

    let mut bytes = Vec::new();
    super::output::json::format_output(&output, &mut bytes).expect("json formatting should work");

    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("output should be valid json");

    assert_eq!(value["type"], "artifact_created");
    assert_eq!(value["artifact_type"], "file");
    assert_eq!(value["artifact_uid"], "artifact-uid");
    assert_eq!(value["filepath"], "outputs/report.txt");
    assert_eq!(value["filename"], "report.txt");
    assert_eq!(value["mime_type"], "text/plain");
    assert_eq!(value["description"], "Build output for the latest run");
    assert_eq!(value["size_bytes"], 42);
}

#[test]
fn json_format_input_omits_filepath_and_description_for_proto_upload_result() {
    let input = AIAgentInput::ActionResult {
        result: AIAgentActionResult {
            id: "tool-call-1".to_string().into(),
            task_id: TaskId::new("task-1".to_string()),
            result: AIAgentActionResultType::UploadArtifact(UploadArtifactResult::Success {
                artifact_uid: "artifact-123".to_string(),
                filepath: None,
                mime_type: "text/plain".to_string(),
                description: None,
                size_bytes: 42,
            }),
        },
        context: Arc::from([]),
    };

    let mut bytes = Vec::new();
    super::output::json::format_input(&input, &mut bytes).expect("json formatting should work");

    let value: serde_json::Value =
        serde_json::from_slice(&bytes).expect("output should be valid json");

    assert_eq!(value["type"], "tool_result");
    assert_eq!(value["tool"], "upload_artifact");
    assert_eq!(value["artifact_uid"], "artifact-123");
    assert_eq!(value["mime_type"], "text/plain");
    assert_eq!(value["size_bytes"], 42);
    assert!(value.get("filepath").is_none());
    assert!(value.get("description").is_none());
}
