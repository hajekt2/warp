use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::command::AcpAgentCommand;
use crate::schema::{
    conservative_initialize_request, AuthenticateRequest, AuthenticateResponse,
    CloseSessionRequest, CloseSessionResponse, ContentBlock, InitializeRequest, InitializeResponse,
    ListSessionsRequest, ListSessionsResponse, LoadSessionRequest, LoadSessionResponse,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, ResumeSessionRequest,
    ResumeSessionResponse, SessionId, SetSessionConfigOptionRequest,
    SetSessionConfigOptionResponse, SetSessionModeRequest, SetSessionModeResponse,
};
use crate::transport::{JsonRpcStdioTransport, JsonRpcTransportError};
use crate::{AgentMessage, JsonRpcErrorObject};

const INITIALIZE_METHOD: &str = "initialize";
const AUTHENTICATE_METHOD: &str = "authenticate";
const SESSION_NEW_METHOD: &str = "session/new";
const SESSION_LOAD_METHOD: &str = "session/load";
const SESSION_RESUME_METHOD: &str = "session/resume";
const SESSION_LIST_METHOD: &str = "session/list";
const SESSION_CLOSE_METHOD: &str = "session/close";
const SESSION_PROMPT_METHOD: &str = "session/prompt";
const SESSION_CANCEL_METHOD: &str = "session/cancel";
const SESSION_SET_CONFIG_OPTION_METHOD: &str = "session/set_config_option";
const SESSION_SET_MODE_METHOD: &str = "session/set_mode";
const DEFAULT_INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_PROMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

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
    prompt_timeout: Duration,
}

impl AcpClient {
    pub fn spawn(config: &AcpAgentCommand) -> Result<Self, AcpClientError> {
        Ok(Self::new(JsonRpcStdioTransport::spawn(config)?))
    }

    #[must_use]
    pub fn new(transport: JsonRpcStdioTransport) -> Self {
        Self {
            transport,
            initialize_timeout: DEFAULT_INITIALIZE_TIMEOUT,
            prompt_timeout: DEFAULT_PROMPT_TIMEOUT,
        }
    }

    #[must_use]
    pub fn with_initialize_timeout(mut self, timeout: Duration) -> Self {
        self.initialize_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_prompt_timeout(mut self, timeout: Duration) -> Self {
        self.prompt_timeout = timeout;
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

    pub fn authenticate(
        &self,
        request: AuthenticateRequest,
    ) -> Result<AuthenticateResponse, AcpClientError> {
        Ok(self.transport.request(AUTHENTICATE_METHOD, request)?)
    }

    pub fn load_session(
        &self,
        request: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, AcpClientError> {
        Ok(self.transport.request(SESSION_LOAD_METHOD, request)?)
    }

    pub fn resume_session(
        &self,
        request: ResumeSessionRequest,
    ) -> Result<ResumeSessionResponse, AcpClientError> {
        Ok(self.transport.request(SESSION_RESUME_METHOD, request)?)
    }

    pub fn list_sessions(
        &self,
        request: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, AcpClientError> {
        Ok(self.transport.request(SESSION_LIST_METHOD, request)?)
    }

    pub fn close_session(
        &self,
        session_id: SessionId,
    ) -> Result<CloseSessionResponse, AcpClientError> {
        Ok(self
            .transport
            .request(SESSION_CLOSE_METHOD, CloseSessionRequest::new(session_id))?)
    }

    pub fn set_session_mode(
        &self,
        request: SetSessionModeRequest,
    ) -> Result<SetSessionModeResponse, AcpClientError> {
        Ok(self.transport.request(SESSION_SET_MODE_METHOD, request)?)
    }

    pub fn set_session_config_option(
        &self,
        request: SetSessionConfigOptionRequest,
    ) -> Result<SetSessionConfigOptionResponse, AcpClientError> {
        Ok(self
            .transport
            .request(SESSION_SET_CONFIG_OPTION_METHOD, request)?)
    }

    pub fn prompt(
        &self,
        session_id: SessionId,
        prompt: Vec<ContentBlock>,
    ) -> Result<PromptResponse, AcpClientError> {
        Ok(self.transport.request_timeout(
            SESSION_PROMPT_METHOD,
            PromptRequest { session_id, prompt },
            self.prompt_timeout,
        )?)
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
        handle_message: F,
    ) -> Result<PromptResponse, AcpClientError>
    where
        F: FnMut(AgentMessage),
    {
        self.prompt_with_agent_message_and_request_handler(
            session_id,
            prompt,
            handle_message,
            deny_unsupported_agent_request,
        )
    }

    pub fn prompt_with_agent_message_and_request_handler<F, G>(
        &self,
        session_id: SessionId,
        prompt: Vec<ContentBlock>,
        mut handle_message: F,
        mut handle_request: G,
    ) -> Result<PromptResponse, AcpClientError>
    where
        F: FnMut(AgentMessage),
        G: FnMut(AgentMessage, &JsonRpcStdioTransport) -> Result<(), JsonRpcTransportError>,
    {
        Ok(self.transport.request_timeout_with_handler(
            SESSION_PROMPT_METHOD,
            PromptRequest { session_id, prompt },
            self.prompt_timeout,
            |message, transport| match message {
                AgentMessage::Notification { .. } => {
                    handle_message(message);
                    Ok(())
                }
                AgentMessage::Request { .. } => handle_request(message, transport),
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
    use std::io::{Cursor, Read, Write};
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

    struct ResponseAfterRequestWrite {
        response: Cursor<Vec<u8>>,
        captured_writes: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for ResponseAfterRequestWrite {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            while self.captured_writes.lock().unwrap().is_empty() {
                std::thread::sleep(Duration::from_millis(1));
            }

            let n = self.response.read(buf)?;
            if n == 0 {
                std::thread::sleep(Duration::from_secs(60));
            }
            Ok(n)
        }
    }

    fn client_with_response(response: &'static [u8]) -> (AcpClient, Arc<Mutex<Vec<u8>>>) {
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        (
            AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
                ResponseAfterRequestWrite {
                    response: Cursor::new(response.to_vec()),
                    captured_writes: Arc::clone(&captured),
                },
                writer,
                None,
            )),
            captured,
        )
    }

    #[test]
    fn initialize_rejects_newer_agent_protocol() {
        let (client, _) = client_with_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":999,"agentCapabilities":{},"agentInfo":{"name":"future"},"authMethods":[]}}
"#,
        );

        let error = client.initialize("Warp").unwrap_err();

        assert!(matches!(
            error,
            AcpClientError::UnsupportedProtocolVersion { .. }
        ));
    }

    #[test]
    fn sends_new_session_request() {
        let (client, captured) = client_with_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{"sessionId":"s1"}}
"#,
        );

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
    fn sends_session_lifecycle_requests() {
        let (client, captured) = client_with_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{"sessions":[{"sessionId":"s1"}]}}
"#,
        );
        let listed = client
            .list_sessions(ListSessionsRequest::default())
            .unwrap();
        assert_eq!(listed.sessions[0].session_id, SessionId::new("s1"));
        assert!(String::from_utf8(captured.lock().unwrap().clone())
            .unwrap()
            .contains("session/list"));

        let (client, captured) = client_with_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{}}
"#,
        );
        let _loaded = client
            .load_session(LoadSessionRequest::new("s1", "/tmp"))
            .unwrap();
        assert!(String::from_utf8(captured.lock().unwrap().clone())
            .unwrap()
            .contains("session/load"));

        let (client, captured) = client_with_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{}}
"#,
        );
        let _resumed = client
            .resume_session(ResumeSessionRequest::new("s1", "/tmp"))
            .unwrap();
        assert!(String::from_utf8(captured.lock().unwrap().clone())
            .unwrap()
            .contains("session/resume"));

        let (client, captured) = client_with_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{}}
"#,
        );
        let _closed = client.close_session(SessionId::new("s1")).unwrap();
        assert!(String::from_utf8(captured.lock().unwrap().clone())
            .unwrap()
            .contains("session/close"));
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

    #[test]
    fn prompt_timeout_defaults_to_long_running_agent_window() {
        let client = AcpClient::new(JsonRpcStdioTransport::from_reader_writer(
            Cursor::new(Vec::new()),
            SharedWriter::default(),
            None,
        ));

        assert_eq!(client.prompt_timeout, Duration::from_secs(30 * 60));
    }
}
