use std::sync::{Arc, Mutex};

use serde_json::Value;
use warp_acp::{AcpAgentCommand, AcpClient, AgentMessage, ContentBlock, NewSessionRequest};

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
