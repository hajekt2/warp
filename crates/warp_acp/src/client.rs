use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::command::AcpAgentCommand;
use crate::schema::{
    conservative_initialize_request, ContentBlock, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, SessionId,
};
use crate::transport::{JsonRpcStdioTransport, JsonRpcTransportError};
use crate::{AgentMessage, JsonRpcErrorObject};

const INITIALIZE_METHOD: &str = "initialize";
const SESSION_NEW_METHOD: &str = "session/new";
const SESSION_PROMPT_METHOD: &str = "session/prompt";
const SESSION_CANCEL_METHOD: &str = "session/cancel";

#[derive(Debug, Error)]
pub enum AcpClientError {
    #[error(transparent)]
    Transport(#[from] JsonRpcTransportError),
    #[error("ACP agent selected unsupported protocol version {agent_version}; client supports {client_version}")]
    UnsupportedProtocolVersion {
        client_version: u16,
        agent_version: u16,
    },
}

pub struct AcpClient {
    transport: JsonRpcStdioTransport,
    initialize_timeout: Duration,
}

impl AcpClient {
    pub fn spawn(config: &AcpAgentCommand) -> Result<Self, AcpClientError> {
        Ok(Self::new(JsonRpcStdioTransport::spawn(config)?))
    }

    #[must_use]
    pub fn new(transport: JsonRpcStdioTransport) -> Self {
        Self {
            transport,
            initialize_timeout: Duration::from_secs(10),
        }
    }

    #[must_use]
    pub fn with_initialize_timeout(mut self, timeout: Duration) -> Self {
        self.initialize_timeout = timeout;
        self
    }

    pub fn initialize(&self, client_name: &str) -> Result<InitializeResponse, AcpClientError> {
        self.initialize_with(conservative_initialize_request(client_name))
    }

    pub fn initialize_with(
        &self,
        request: InitializeRequest,
    ) -> Result<InitializeResponse, AcpClientError> {
        let client_version = request.protocol_version.as_u16();
        let response: InitializeResponse =
            self.transport
                .request_timeout(INITIALIZE_METHOD, request, self.initialize_timeout)?;
        let agent_version = response.protocol_version.as_u16();
        if agent_version > client_version {
            return Err(AcpClientError::UnsupportedProtocolVersion {
                client_version,
                agent_version,
            });
        }
        Ok(response)
    }

    pub fn new_session(
        &self,
        request: NewSessionRequest,
    ) -> Result<NewSessionResponse, AcpClientError> {
        Ok(self.transport.request(SESSION_NEW_METHOD, request)?)
    }

    pub fn prompt(
        &self,
        session_id: SessionId,
        prompt: Vec<ContentBlock>,
    ) -> Result<PromptResponse, AcpClientError> {
        Ok(self
            .transport
            .request(SESSION_PROMPT_METHOD, PromptRequest { session_id, prompt })?)
    }

    /// Send a prompt while conservatively denying agent-initiated client requests.
    ///
    /// Warp advertises no filesystem or terminal capability until those requests are
    /// routed through native approval UI. If an agent still calls them, respond rather
    /// than leaving the protocol turn hanging.
    pub fn prompt_with_conservative_request_handling(
        &self,
        session_id: SessionId,
        prompt: Vec<ContentBlock>,
    ) -> Result<PromptResponse, AcpClientError> {
        self.prompt_with_agent_message_handler(session_id, prompt, |_| {})
    }

    /// Send a prompt and surface agent notifications while denying unsupported
    /// agent-to-client requests.
    pub fn prompt_with_agent_message_handler<F>(
        &self,
        session_id: SessionId,
        prompt: Vec<ContentBlock>,
        mut handle_message: F,
    ) -> Result<PromptResponse, AcpClientError>
    where
        F: FnMut(AgentMessage),
    {
        Ok(self.transport.request_timeout_with_handler(
            SESSION_PROMPT_METHOD,
            PromptRequest { session_id, prompt },
            Duration::from_secs(30),
            |message, transport| match message {
                AgentMessage::Notification { .. } => {
                    handle_message(message);
                    Ok(())
                }
                AgentMessage::Request { .. } => deny_unsupported_agent_request(message, transport),
            },
        )?)
    }

    pub fn cancel(&self, session_id: SessionId) -> Result<(), AcpClientError> {
        self.transport.notify(
            SESSION_CANCEL_METHOD,
            serde_json::json!({ "sessionId": session_id }),
        )?;
        Ok(())
    }

    pub fn recv_agent_message(
        &self,
        timeout: Duration,
    ) -> Result<Option<AgentMessage>, AcpClientError> {
        Ok(self.transport.recv_message(timeout)?)
    }

    pub fn respond_success(
        &self,
        id: crate::JsonRpcId,
        result: Value,
    ) -> Result<(), AcpClientError> {
        Ok(self.transport.respond_result(id, result)?)
    }

    pub fn respond_error(
        &self,
        id: crate::JsonRpcId,
        code: i64,
        message: impl Into<String>,
    ) -> Result<(), AcpClientError> {
        Ok(self
            .transport
            .respond_error(id, crate::JsonRpcErrorObject::new(code, message))?)
    }
}

fn deny_unsupported_agent_request(
    message: AgentMessage,
    transport: &JsonRpcStdioTransport,
) -> Result<(), JsonRpcTransportError> {
    let AgentMessage::Request { id, method, .. } = message else {
        return Ok(());
    };

    match method.as_str() {
        // We have not surfaced Warp's permission UI yet. A cancellation-shaped
        // response is safer than implicitly granting or hanging the agent.
        "session/request_permission" => transport.respond_result(
            id,
            serde_json::json!({
                "outcome": {
                    "outcome": "cancelled"
                }
            }),
        ),
        // These methods are disabled in conservative ClientCapabilities. Agents
        // should not call them; if they do, return a JSON-RPC error immediately.
        "fs/read_text_file"
        | "fs/write_text_file"
        | "terminal/create"
        | "terminal/output"
        | "terminal/release"
        | "terminal/wait_for_exit"
        | "terminal/kill" => transport.respond_error(
            id,
            JsonRpcErrorObject::new(
                -32001,
                format!("ACP client method `{method}` is not enabled"),
            ),
        ),
        _ => transport.respond_error(
            id,
            JsonRpcErrorObject::new(-32601, format!("Unknown ACP client method `{method}`")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;
    use crate::schema::StopReason;
    use crate::transport::JsonRpcStdioTransport;

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn initialize_rejects_newer_agent_protocol() {
        let input = Cursor::new(br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":999,"agentCapabilities":{},"agentInfo":{"name":"future"},"authMethods":[]}}
"#.to_vec());
        let client = AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
            input,
            SharedWriter::default(),
            None,
        ));

        let error = client.initialize("Warp").unwrap_err();

        assert!(matches!(
            error,
            AcpClientError::UnsupportedProtocolVersion { .. }
        ));
    }

    #[test]
    fn sends_new_session_request() {
        let input = Cursor::new(
            br#"{"jsonrpc":"2.0","id":1,"result":{"sessionId":"s1"}}
"#
            .to_vec(),
        );
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let client = AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
            input, writer, None,
        ));

        let response = client.new_session(NewSessionRequest::new("/tmp")).unwrap();

        assert_eq!(response.session_id, SessionId::new("s1"));
        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains("session/new"));
        assert!(written.contains("/tmp"));
    }

    #[test]
    fn sends_cancel_notification() {
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let client = AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
            Cursor::new(Vec::new()),
            writer,
            None,
        ));

        client.cancel(SessionId::new("s1")).unwrap();

        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains("session/cancel"));
        assert!(written.contains("s1"));
        assert!(!written.contains("\"id\""));
    }

    #[test]
    fn prompt_denies_permission_request_before_response() {
        let input = Cursor::new(
            br#"{"jsonrpc":"2.0","id":99,"method":"session/request_permission","params":{"reason":"test"}}
{"jsonrpc":"2.0","id":1,"result":{"stopReason":"end_turn"}}
"#
            .to_vec(),
        );
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let client = AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
            input, writer, None,
        ));

        let response = client
            .prompt_with_conservative_request_handling(
                SessionId::new("s1"),
                vec![ContentBlock::text("hello")],
            )
            .unwrap();

        assert_eq!(response.stop_reason, StopReason::EndTurn);
        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains(r#""id":99"#));
        assert!(written.contains("cancelled"));
    }

    #[test]
    fn prompt_errors_disabled_filesystem_request_before_response() {
        let input = Cursor::new(
            br#"{"jsonrpc":"2.0","id":99,"method":"fs/write_text_file","params":{"path":"/tmp/x","content":"x"}}
{"jsonrpc":"2.0","id":1,"result":{"stopReason":"end_turn"}}
"#
            .to_vec(),
        );
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let client = AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
            input, writer, None,
        ));

        let response = client
            .prompt_with_conservative_request_handling(
                SessionId::new("s1"),
                vec![ContentBlock::text("hello")],
            )
            .unwrap();

        assert_eq!(response.stop_reason, StopReason::EndTurn);
        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains(r#""id":99"#));
        assert!(written.contains(r#""code":-32001"#));
        assert!(written.contains("not enabled"));
    }

    #[test]
    fn prompt_round_trips_response() {
        let input = Cursor::new(
            br#"{"jsonrpc":"2.0","id":1,"result":{"stopReason":"end_turn"}}
"#
            .to_vec(),
        );
        let client = AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
            input,
            SharedWriter::default(),
            None,
        ));

        let response = client
            .prompt(SessionId::new("s1"), vec![ContentBlock::text("hello")])
            .unwrap();

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({"stopReason": "end_turn"})
        );
    }
}
