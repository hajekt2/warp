use std::sync::{Arc, Mutex};

use serde_json::Value;
use warp_acp::{
    AcpAgentCommand, AcpClient, AcpEnvironmentVariable, AgentMessage, AuthenticateRequest,
    ContentBlock, LoadSessionRequest, NewSessionRequest,
};

#[test]
fn echo_fixture_completes_initialize_session_and_prompt() {
    let exe = env!("CARGO_BIN_EXE_echo_acp_agent");
    let command = AcpAgentCommand::new(exe);
    let client = AcpClient::spawn(&command).expect("spawn echo ACP fixture");

    let initialize = client.initialize("Warp test").expect("initialize");
    assert_eq!(initialize.protocol_version.as_u16(), 1);

    let session = client
        .new_session(NewSessionRequest::new(std::env::current_dir().unwrap()))
        .expect("new session");

    let updates: Arc<Mutex<Vec<Value>>> = Arc::default();
    let updates_for_handler = Arc::clone(&updates);
    let response = client
        .prompt_with_agent_message_handler(
            session.session_id,
            vec![ContentBlock::text("hello")],
            move |message| {
                if let AgentMessage::Notification { method, params } = message {
                    if method == "session/update" {
                        updates_for_handler.lock().unwrap().push(params.clone());
                    }
                }
            },
        )
        .expect("prompt");

    assert_eq!(
        serde_json::to_value(response).unwrap()["stopReason"],
        "end_turn"
    );
    let updates = updates.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0]["update"]["sessionUpdate"], "agent_message_chunk");
    assert_eq!(updates[0]["update"]["content"]["text"], "echo: hello");
}

#[test]
fn echo_fixture_accepts_multiple_prompts_on_one_session() {
    let exe = env!("CARGO_BIN_EXE_echo_acp_agent");
    let command = AcpAgentCommand::new(exe);
    let client = AcpClient::spawn(&command).expect("spawn echo ACP fixture");

    client.initialize("Warp test").expect("initialize");
    let session = client
        .new_session(NewSessionRequest::new(std::env::current_dir().unwrap()))
        .expect("new session");

    let updates: Arc<Mutex<Vec<Value>>> = Arc::default();
    for prompt in ["first", "second"] {
        let updates_for_handler = Arc::clone(&updates);
        client
            .prompt_with_agent_message_handler(
                session.session_id.clone(),
                vec![ContentBlock::text(prompt)],
                move |message| {
                    if let AgentMessage::Notification { method, params } = message {
                        if method == "session/update" {
                            updates_for_handler.lock().unwrap().push(params.clone());
                        }
                    }
                },
            )
            .expect("prompt on existing session");
    }

    let updates = updates.lock().unwrap();
    assert_eq!(updates.len(), 2);
    assert!(updates
        .iter()
        .all(|update| update["sessionId"] == "echo-session"));
    assert_eq!(updates[0]["update"]["content"]["text"], "echo: first");
    assert_eq!(updates[1]["update"]["content"]["text"], "echo: second");
}

#[test]
fn echo_fixture_loads_existing_session() {
    let exe = env!("CARGO_BIN_EXE_echo_acp_agent");
    let command = AcpAgentCommand::new(exe);
    let client = AcpClient::spawn(&command).expect("spawn echo ACP fixture");

    client.initialize("Warp test").expect("initialize");
    client
        .load_session(LoadSessionRequest::new(
            "persisted-echo-session",
            std::env::current_dir().unwrap(),
        ))
        .expect("load session");

    let updates: Arc<Mutex<Vec<Value>>> = Arc::default();
    let updates_for_handler = Arc::clone(&updates);
    client
        .prompt_with_agent_message_handler(
            warp_acp::SessionId::new("persisted-echo-session"),
            vec![ContentBlock::text("after restart")],
            move |message| {
                if let AgentMessage::Notification { method, params } = message {
                    if method == "session/update" {
                        updates_for_handler.lock().unwrap().push(params.clone());
                    }
                }
            },
        )
        .expect("prompt after load");

    let updates = updates.lock().unwrap();
    assert_eq!(updates[0]["sessionId"], "persisted-echo-session");
    assert_eq!(
        updates[0]["update"]["content"]["text"],
        "echo: after restart"
    );
}

#[test]
fn echo_fixture_authenticates_before_new_session_when_required() {
    let exe = env!("CARGO_BIN_EXE_echo_acp_agent");
    let command =
        AcpAgentCommand::new(exe).env([AcpEnvironmentVariable::new("ECHO_ACP_REQUIRE_AUTH", "1")]);
    let client = AcpClient::spawn(&command).expect("spawn echo ACP fixture");

    let initialize = client.initialize("Warp test").expect("initialize");
    let method_id = initialize.auth_methods[0]["methodId"].as_str().unwrap();
    client
        .authenticate(AuthenticateRequest::new(method_id))
        .expect("authenticate");

    let session = client
        .new_session(NewSessionRequest::new(std::env::current_dir().unwrap()))
        .expect("new session after auth");

    assert_eq!(session.session_id.0, "echo-session");
}
