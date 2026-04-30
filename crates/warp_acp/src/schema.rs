use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Stable ACP protocol version identifier.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct ProtocolVersion(u16);

impl ProtocolVersion {
    pub const V0: Self = Self(0);
    pub const V1: Self = Self(1);
    pub const LATEST: Self = Self::V1;

    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = ProtocolVersion;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an ACP protocol version number")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let version = u16::try_from(value)
                    .map_err(|_| E::custom(format!("protocol version {value} is too large")))?;
                Ok(ProtocolVersion(version))
            }

            fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(ProtocolVersion::V0)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

/// Metadata about the ACP client/agent implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Implementation {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl Implementation {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
        }
    }

    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemCapabilities {
    #[serde(default)]
    pub read_text_file: bool,
    #[serde(default)]
    pub write_text_file: bool,
}

impl FileSystemCapabilities {
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn read_only() -> Self {
        Self {
            read_text_file: true,
            write_text_file: false,
        }
    }

    #[must_use]
    pub fn read_write() -> Self {
        Self {
            read_text_file: true,
            write_text_file: true,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default)]
    pub fs: FileSystemCapabilities,
    #[serde(default)]
    pub terminal: bool,
}

impl ClientCapabilities {
    #[must_use]
    pub fn conservative() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_file_system(mut self, fs: FileSystemCapabilities) -> Self {
        self.fs = fs;
        self
    }

    #[must_use]
    pub fn with_terminal(mut self, terminal: bool) -> Self {
        self.terminal = terminal;
        self
    }
}

/// ACP `initialize` request sent by Warp as the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequest {
    pub protocol_version: ProtocolVersion,
    #[serde(default)]
    pub client_capabilities: ClientCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<Implementation>,
}

impl InitializeRequest {
    #[must_use]
    pub fn new(
        client_info: Option<Implementation>,
        client_capabilities: ClientCapabilities,
    ) -> Self {
        Self {
            protocol_version: ProtocolVersion::LATEST,
            client_capabilities,
            client_info,
        }
    }
}

#[must_use]
pub fn conservative_initialize_request(client_name: impl Into<String>) -> InitializeRequest {
    InitializeRequest::new(
        Some(Implementation::new(client_name.into()).with_version(env!("CARGO_PKG_VERSION"))),
        ClientCapabilities::conservative(),
    )
}

/// ACP `initialize` response returned by the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub protocol_version: ProtocolVersion,
    #[serde(default)]
    pub agent_capabilities: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_info: Option<Implementation>,
    #[serde(default)]
    pub auth_methods: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

impl SessionId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// ACP MCP server configuration forwarded only after explicit allowlist consent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpServer {
    /// Stdio is untagged in the official ACP schema because every agent must support it.
    #[serde(untagged)]
    Stdio(McpServerStdio),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStdio {
    pub name: String,
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<AcpEnvironmentEntry>,
}

impl McpServerStdio {
    #[must_use]
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }
}

/// ACP `session/new` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionRequest {
    pub cwd: PathBuf,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

impl NewSessionRequest {
    #[must_use]
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            mcp_servers: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_mcp_servers(mut self, mcp_servers: Vec<McpServer>) -> Self {
        self.mcp_servers = mcp_servers;
        self
    }
}

/// ACP `session/new` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionResponse {
    pub session_id: SessionId,
    #[serde(default)]
    pub available_modes: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextContent {
    pub text: String,
}

impl TextContent {
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text(TextContent),
}

impl ContentBlock {
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(TextContent::new(text))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateRequest {
    pub method_id: String,
}

impl AuthenticateRequest {
    #[must_use]
    pub fn new(method_id: impl Into<String>) -> Self {
        Self {
            method_id: method_id.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticateResponse {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionModeRequest {
    pub session_id: SessionId,
    pub mode_id: String,
}

impl SetSessionModeRequest {
    #[must_use]
    pub fn new(session_id: SessionId, mode_id: impl Into<String>) -> Self {
        Self {
            session_id,
            mode_id: mode_id.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetSessionModeResponse {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionConfigOptionRequest {
    pub session_id: SessionId,
    pub config_id: String,
    pub value: String,
}

impl SetSessionConfigOptionRequest {
    #[must_use]
    pub fn new(
        session_id: SessionId,
        config_id: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            session_id,
            config_id: config_id.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSessionConfigOptionResponse {
    #[serde(default)]
    pub config_options: Vec<Value>,
}

/// ACP `session/load` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionRequest {
    pub session_id: SessionId,
    pub cwd: PathBuf,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

impl LoadSessionRequest {
    #[must_use]
    pub fn new(session_id: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            session_id: SessionId::new(session_id),
            cwd: cwd.into(),
            mcp_servers: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_mcp_servers(mut self, mcp_servers: Vec<McpServer>) -> Self {
        self.mcp_servers = mcp_servers;
        self
    }
}

/// ACP `session/load` response. Optional state is left schema-shaped for forward compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadSessionResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Vec<Value>>,
}

/// ACP `session/resume` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSessionRequest {
    pub session_id: SessionId,
    pub cwd: PathBuf,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
}

impl ResumeSessionRequest {
    #[must_use]
    pub fn new(session_id: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            session_id: SessionId::new(session_id),
            cwd: cwd.into(),
            mcp_servers: Vec::new(),
        }
    }
}

/// ACP `session/resume` response.
pub type ResumeSessionResponse = LoadSessionResponse;

/// ACP `session/list` request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// ACP session metadata in `session/list` responses. Kept open-ended via raw JSON fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedSession {
    pub session_id: SessionId,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSessionsResponse {
    #[serde(default)]
    pub sessions: Vec<ListedSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseSessionRequest {
    pub session_id: SessionId,
}

impl CloseSessionRequest {
    #[must_use]
    pub fn new(session_id: SessionId) -> Self {
        Self { session_id }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseSessionResponse {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextFileRequest {
    pub session_id: SessionId,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadTextFileResponse {
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteTextFileRequest {
    pub session_id: SessionId,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteTextFileResponse {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestPermissionRequest {
    pub session_id: SessionId,
    pub tool_call: Value,
    #[serde(default)]
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RequestPermissionOutcome {
    Cancelled,
    Selected {
        #[serde(rename = "optionId")]
        option_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestPermissionResponse {
    pub outcome: RequestPermissionOutcome,
}

impl RequestPermissionResponse {
    #[must_use]
    pub fn cancelled() -> Self {
        Self {
            outcome: RequestPermissionOutcome::Cancelled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalRequest {
    pub session_id: SessionId,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Vec<AcpEnvironmentEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_byte_limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpEnvironmentEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTerminalResponse {
    pub terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRefRequest {
    pub session_id: SessionId,
    pub terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalExitStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputResponse {
    pub output: String,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_status: Option<TerminalExitStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitForTerminalExitResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseTerminalResponse {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillTerminalResponse {}

/// ACP prompt stop reason. Unknown future values are preserved as strings at the
/// serde boundary by callers that need more detail; v1 code only branches on
/// cancellation versus completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    Cancelled,
    MaxTokens,
    Refusal,
}

/// ACP `session/prompt` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptResponse {
    pub stop_reason: StopReason,
}

/// ACP `session/prompt` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    pub session_id: SessionId,
    pub prompt: Vec<ContentBlock>,
}

impl PromptRequest {
    #[must_use]
    pub fn new(session_id: impl Into<String>, prompt: Vec<ContentBlock>) -> Self {
        Self {
            session_id: SessionId::new(session_id),
            prompt,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_serializes_acp_v1_and_conservative_capabilities() {
        let request = conservative_initialize_request("Warp");
        let value = serde_json::to_value(request).unwrap();

        assert_eq!(value["protocolVersion"], 1);
        assert_eq!(value["clientCapabilities"]["fs"]["readTextFile"], false);
        assert_eq!(value["clientCapabilities"]["fs"]["writeTextFile"], false);
        assert_eq!(value["clientCapabilities"]["terminal"], false);
        assert_eq!(value["clientInfo"]["name"], "Warp");
        assert_eq!(value["clientInfo"]["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn protocol_version_string_deserializes_as_v0_compatibility_fallback() {
        let version: ProtocolVersion = serde_json::from_str("\"0.1\"").unwrap();

        assert_eq!(version, ProtocolVersion::V0);
    }

    #[test]
    fn prompt_serializes_text_content_block() {
        let prompt = PromptRequest::new("session-1", vec![ContentBlock::text("hello")]);
        let value = serde_json::to_value(prompt).unwrap();

        assert_eq!(value["sessionId"], "session-1");
        assert_eq!(value["prompt"][0]["type"], "text");
        assert_eq!(value["prompt"][0]["text"], "hello");
    }

    #[test]
    fn request_permission_selected_serializes_official_option_id_field() {
        let response = RequestPermissionResponse {
            outcome: RequestPermissionOutcome::Selected {
                option_id: "approve".to_string(),
            },
        };
        let value = serde_json::to_value(response).unwrap();

        assert_eq!(value["outcome"]["outcome"], "selected");
        assert_eq!(value["outcome"]["optionId"], "approve");
    }

    #[test]
    fn session_load_serializes_cwd_and_session_id() {
        let value = serde_json::to_value(LoadSessionRequest::new("s1", "/tmp/work")).unwrap();

        assert_eq!(value["sessionId"], "s1");
        assert_eq!(value["cwd"], "/tmp/work");
    }

    #[test]
    fn mcp_stdio_server_serializes_as_acp_stdio_server() {
        let request = NewSessionRequest::new("/tmp").with_mcp_servers(vec![McpServer::Stdio(
            McpServerStdio::new("test", "/bin/echo").args(["hello"]),
        )]);
        let value = serde_json::to_value(request).unwrap();

        assert!(value["mcpServers"][0].get("type").is_none());
        assert_eq!(value["mcpServers"][0]["name"], "test");
        assert_eq!(value["mcpServers"][0]["command"], "/bin/echo");
        assert_eq!(value["mcpServers"][0]["args"][0], "hello");
        assert_eq!(value["mcpServers"][0]["env"].as_array().unwrap().len(), 0);
    }
}
