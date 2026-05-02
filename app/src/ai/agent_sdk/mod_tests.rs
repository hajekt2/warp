use super::{command_requires_auth, command_to_telemetry_event, reconcile_task_harness};
use serde_json::json;
use warp_cli::{
    agent::{Harness, HiddenComputerUseArgs, PromptArg, RunAgentArgs, SnapshotArgs},
    artifact::{ArtifactCommand, DownloadArtifactArgs, GetArtifactArgs, UploadArtifactArgs},
    config_file::ConfigFileArgs,
    model::ModelArgs,
    share::ShareArgs,
    task::{MessageCommand, MessageSendArgs, MessageWatchArgs, TaskCommand},
    CliCommand,
};
use warp_core::telemetry::TelemetryEvent;

const TASK_ID: &str = "00000000-0000-0000-0000-000000000001";

fn run_agent_args_for_harness(harness: Harness) -> RunAgentArgs {
    RunAgentArgs {
        prompt_arg: PromptArg {
            prompt: Some("hello".to_string()),
            saved_prompt: None,
        },
        model: ModelArgs::default(),
        config_file: ConfigFileArgs::default(),
        skill: None,
        name: None,
        cwd: None,
        gui: false,
        share: ShareArgs { share: None },
        mcp_specs: Vec::new(),
        mcp_servers: Vec::new(),
        environment: None,
        idle_on_complete: None,
        snapshot: SnapshotArgs {
            no_snapshot: false,
            snapshot_upload_timeout: None,
            snapshot_script_timeout: None,
        },
        task_id: None,
        sandboxed: false,
        bedrock_inference_role: None,
        computer_use: HiddenComputerUseArgs::default(),
        conversation: None,
        profile: None,
        harness,
    }
}

#[test]
fn logout_does_not_require_auth() {
    assert!(!command_requires_auth(&CliCommand::Logout));
}

#[test]
fn login_does_not_require_auth() {
    assert!(!command_requires_auth(&CliCommand::Login));
}

#[test]
fn local_acp_agent_run_does_not_require_auth() {
    assert!(!command_requires_auth(&CliCommand::Agent(
        warp_cli::agent::AgentCommand::Run(run_agent_args_for_harness(Harness::Acp)),
    )));
}

#[test]
fn oz_agent_run_still_requires_auth() {
    assert!(command_requires_auth(&CliCommand::Agent(
        warp_cli::agent::AgentCommand::Run(run_agent_args_for_harness(Harness::Oz)),
    )));
}

#[test]
fn legacy_opencode_agent_run_does_not_require_auth() {
    assert!(!command_requires_auth(&CliCommand::Agent(
        warp_cli::agent::AgentCommand::Run(run_agent_args_for_harness(Harness::OpenCode)),
    )));
}

#[test]
fn acp_agent_run_with_saved_prompt_requires_auth() {
    let mut args = run_agent_args_for_harness(Harness::Acp);
    args.prompt_arg.prompt = None;
    args.prompt_arg.saved_prompt = Some("saved-prompt-id".to_string());

    assert!(command_requires_auth(&CliCommand::Agent(
        warp_cli::agent::AgentCommand::Run(args),
    )));
}

#[test]
fn acp_agent_run_with_warp_server_features_requires_auth() {
    let mut args = run_agent_args_for_harness(Harness::Acp);
    args.share.share = Some(Vec::new());

    assert!(command_requires_auth(&CliCommand::Agent(
        warp_cli::agent::AgentCommand::Run(args),
    )));
}

#[test]
fn artifact_download_requires_auth() {
    assert!(command_requires_auth(&CliCommand::Artifact(
        ArtifactCommand::Download(DownloadArtifactArgs {
            artifact_uid: "artifact-123".to_string(),
            out: None,
        },)
    )));
}

#[test]
fn run_message_send_requires_auth() {
    assert!(command_requires_auth(&CliCommand::Run(
        TaskCommand::Message(MessageCommand::Send(MessageSendArgs {
            to: vec!["run-456".to_string()],
            subject: "subject".to_string(),
            body: "body".to_string(),
            sender_run_id: "run-123".to_string(),
        }),)
    )));
}

#[test]
fn artifact_get_requires_auth() {
    assert!(command_requires_auth(&CliCommand::Artifact(
        ArtifactCommand::Get(GetArtifactArgs {
            artifact_uid: "artifact-123".to_string(),
        },)
    )));
}

#[test]
fn artifact_upload_requires_auth() {
    assert!(command_requires_auth(&CliCommand::Artifact(
        ArtifactCommand::Upload(UploadArtifactArgs {
            path: "artifact.txt".into(),
            run_id: Some("run-123".to_string()),
            conversation_id: None,
            description: None,
        },)
    )));
}

#[test]
#[serial_test::serial]
fn run_message_send_telemetry_uses_canonical_harness_from_env() {
    std::env::set_var("OZ_HARNESS", "  CLAUDE  ");
    let event = command_to_telemetry_event(&CliCommand::Run(TaskCommand::Message(
        MessageCommand::Send(MessageSendArgs {
            to: vec!["run-456".to_string()],
            subject: "subject".to_string(),
            body: "body".to_string(),
            sender_run_id: "run-123".to_string(),
        }),
    )));
    std::env::remove_var("OZ_HARNESS");

    assert_eq!(event.payload(), Some(json!({ "harness": "claude" })));
}

#[test]
#[serial_test::serial]
fn run_message_send_telemetry_supports_claude_code_alias() {
    std::env::set_var("OZ_HARNESS", "CLAUDE_CODE");
    let event = command_to_telemetry_event(&CliCommand::Run(TaskCommand::Message(
        MessageCommand::Send(MessageSendArgs {
            to: vec!["run-456".to_string()],
            subject: "subject".to_string(),
            body: "body".to_string(),
            sender_run_id: "run-123".to_string(),
        }),
    )));
    std::env::remove_var("OZ_HARNESS");

    assert_eq!(event.payload(), Some(json!({ "harness": "claude" })));
}

#[test]
#[serial_test::serial]
fn run_message_send_telemetry_supports_opencode_harness() {
    std::env::set_var("OZ_HARNESS", "opencode");
    let event = command_to_telemetry_event(&CliCommand::Run(TaskCommand::Message(
        MessageCommand::Send(MessageSendArgs {
            to: vec!["run-456".to_string()],
            subject: "subject".to_string(),
            body: "body".to_string(),
            sender_run_id: "run-123".to_string(),
        }),
    )));
    std::env::remove_var("OZ_HARNESS");

    assert_eq!(event.payload(), Some(json!({ "harness": "opencode" })));
}

#[test]
#[serial_test::serial]
fn run_message_send_telemetry_defaults_to_unknown_harness() {
    std::env::remove_var("OZ_HARNESS");
    let event = command_to_telemetry_event(&CliCommand::Run(TaskCommand::Message(
        MessageCommand::Send(MessageSendArgs {
            to: vec!["run-456".to_string()],
            subject: "subject".to_string(),
            body: "body".to_string(),
            sender_run_id: "run-123".to_string(),
        }),
    )));

    assert_eq!(event.payload(), Some(json!({ "harness": "unknown" })));
}

#[test]
fn reconcile_task_harness_adopts_task_harness_when_cli_uses_default() {
    let mut selected_harness = Harness::Oz;
    let harness = reconcile_task_harness(TASK_ID, &mut selected_harness, Harness::Claude)
        .expect("default harness should adopt task harness");

    assert_eq!(selected_harness, Harness::Claude);
    assert_eq!(harness.harness(), Harness::Claude);
}

#[test]
fn reconcile_task_harness_allows_matching_explicit_harness() {
    let mut selected_harness = Harness::Claude;
    let harness = reconcile_task_harness(TASK_ID, &mut selected_harness, Harness::Claude)
        .expect("matching harness should succeed");

    assert_eq!(selected_harness, Harness::Claude);
    assert_eq!(harness.harness(), Harness::Claude);
}

#[test]
fn reconcile_task_harness_rejects_explicit_mismatch() {
    let mut selected_harness = Harness::Gemini;
    let err = reconcile_task_harness(TASK_ID, &mut selected_harness, Harness::Claude)
        .expect_err("mismatched harness should fail");

    assert_eq!(selected_harness, Harness::Gemini);
    assert!(err.to_string().contains("Task"));
    assert!(err.to_string().contains("--harness gemini"));
    assert!(err.to_string().contains("claude"));
}

#[test]
#[serial_test::serial]
fn run_message_watch_telemetry_defaults_to_unknown_harness() {
    std::env::remove_var("OZ_HARNESS");
    let event = command_to_telemetry_event(&CliCommand::Run(TaskCommand::Message(
        MessageCommand::Watch(MessageWatchArgs {
            run_id: "run-123".to_string(),
            since_sequence: 0,
        }),
    )));

    assert_eq!(event.payload(), Some(json!({ "harness": "unknown" })));
}
