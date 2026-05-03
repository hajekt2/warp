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
    /// HTTP transport is available only when the agent advertises `mcpCapabilities.http`.
    Http(McpServerHttp),
    /// Server-Sent Events transport is available only when the agent advertises
    /// `mcpCapabilities.sse`.
    Sse(McpServerSse),
    /// Stdio is untagged in the official ACP schema because every agent must support it.
    #[serde(untagged)]
    Stdio(McpServerStdio),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerHttp {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<AcpHttpHeader>,
}

impl McpServerHttp {
    #[must_use]
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            headers: Vec::new(),
        }
    }

    #[must_use]
    pub fn headers(mut self, headers: impl IntoIterator<Item = AcpHttpHeader>) -> Self {
        self.headers = headers.into_iter().collect();
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerSse {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<AcpHttpHeader>,
}

impl McpServerSse {
    #[must_use]
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            headers: Vec::new(),
        }
    }

    #[must_use]
    pub fn headers(mut self, headers: impl IntoIterator<Item = AcpHttpHeader>) -> Self {
        self.headers = headers.into_iter().collect();
        self
    }
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

/// ACP `session/update` notification payload.
///
/// The upstream protocol is intentionally open-ended: implementations may add new
/// update kinds without a client upgrade. Warp parses the update discriminator into
/// known variants and preserves unknown payloads so forward-compatible agents do
/// not break the session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdateNotification {
    pub session_id: SessionId,
    pub update: SessionUpdate,
}

/// Typed ACP session update stream item.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionUpdate {
    AgentMessageChunk {
        text: String,
    },
    AgentThoughtChunk {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        args: Value,
    },
    ToolCallUpdate {
        id: String,
        status: Option<String>,
        args: Option<Value>,
        output: Option<String>,
    },
    Plan {
        content: String,
    },
    CurrentModeUpdate {
        mode: Value,
    },
    AvailableCommandsUpdate {
        commands: Vec<Value>,
    },
    Unknown {
        method: String,
        params: Value,
    },
}

impl SessionUpdate {
    #[must_use]
    pub fn from_notification_params(params: &Value) -> Option<Self> {
        if let Some(update) = params.get("update") {
            return serde_json::from_value(update.clone()).ok();
        }
        serde_json::from_value(params.clone()).ok()
    }

    #[must_use]
    pub fn agent_message_chunk(text: impl Into<String>) -> Self {
        Self::AgentMessageChunk { text: text.into() }
    }

    #[must_use]
    pub fn agent_thought_chunk(text: impl Into<String>) -> Self {
        Self::AgentThoughtChunk { text: text.into() }
    }

    fn method(&self) -> &'static str {
        match self {
            Self::AgentMessageChunk { .. } => "agent_message_chunk",
            Self::AgentThoughtChunk { .. } => "agent_thought_chunk",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolCallUpdate { .. } => "tool_call_update",
            Self::Plan { .. } => "plan",
            Self::CurrentModeUpdate { .. } => "current_mode_update",
            Self::AvailableCommandsUpdate { .. } => "available_commands_update",
            Self::Unknown { .. } => "unknown",
        }
    }
}

impl Serialize for SessionUpdate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut value = match self {
            Self::AgentMessageChunk { text } | Self::AgentThoughtChunk { text } => {
                serde_json::json!({ "sessionUpdate": self.method(), "text": text })
            }
            Self::ToolCall { id, name, args } => serde_json::json!({
                "sessionUpdate": "tool_call",
                "toolCall": { "id": id, "name": name, "args": args }
            }),
            Self::ToolCallUpdate {
                id,
                status,
                args,
                output,
            } => serde_json::json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": id,
                "status": status,
                "args": args,
                "output": output,
            }),
            Self::Plan { content } => {
                serde_json::json!({ "sessionUpdate": "plan", "content": content })
            }
            Self::CurrentModeUpdate { mode } => serde_json::json!({
                "sessionUpdate": "current_mode_update",
                "mode": mode,
            }),
            Self::AvailableCommandsUpdate { commands } => serde_json::json!({
                "sessionUpdate": "available_commands_update",
                "commands": commands,
            }),
            Self::Unknown { method, params } => {
                let mut value = params.clone();
                if let Value::Object(map) = &mut value {
                    map.entry("sessionUpdate".to_string())
                        .or_insert_with(|| Value::String(method.clone()));
                }
                value
            }
        };
        if let Value::Object(map) = &mut value {
            map.retain(|_, value| !value.is_null());
        }
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SessionUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let Value::Object(map) = &value else {
            return Ok(Self::Unknown {
                method: "<invalid>".to_string(),
                params: value,
            });
        };
        let raw_method = map
            .get("sessionUpdate")
            .or_else(|| map.get("type"))
            .or_else(|| map.get("kind"))
            .or_else(|| map.get("method"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let normalized = normalize_update_method(raw_method);
        let text = || extract_text(&value).unwrap_or_default();
        match normalized.as_str() {
            "agentmessagechunk" | "agentmessagedelta" | "agentmessage" | "text" => {
                Ok(Self::AgentMessageChunk { text: text() })
            }
            "agentthoughtchunk" | "agentthoughtdelta" | "agentthought" | "thinking" => {
                Ok(Self::AgentThoughtChunk { text: text() })
            }
            "toolcall" => {
                let tool = map.get("toolCall").unwrap_or(&value);
                Ok(Self::ToolCall {
                    id: string_field(tool, &["id", "toolCallId"])
                        .unwrap_or_else(|| "unknown".to_string()),
                    name: string_field(tool, &["name", "title", "toolName"])
                        .unwrap_or_else(|| "tool".to_string()),
                    args: tool
                        .get("args")
                        .or_else(|| tool.get("input"))
                        .cloned()
                        .unwrap_or(Value::Null),
                })
            }
            "toolcallupdate" | "toolcallstatus" | "toolcallresult" => Ok(Self::ToolCallUpdate {
                id: string_field(&value, &["toolCallId", "id"])
                    .unwrap_or_else(|| "unknown".to_string()),
                status: string_field(&value, &["status", "state"]),
                args: map.get("args").or_else(|| map.get("input")).cloned(),
                output: string_field(&value, &["output", "result", "content", "text"]),
            }),
            "plan" => Ok(Self::Plan {
                content: string_field(&value, &["content", "text", "markdown"]).unwrap_or_default(),
            }),
            "currentmodeupdate" | "currentmode" => Ok(Self::CurrentModeUpdate {
                mode: map.get("mode").cloned().unwrap_or(Value::Null),
            }),
            "availablecommandsupdate" | "availablecommands" => Ok(Self::AvailableCommandsUpdate {
                commands: map
                    .get("commands")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            }),
            _ => Ok(Self::Unknown {
                method: raw_method.to_string(),
                params: value,
            }),
        }
    }
}

fn normalize_update_method(method: &str) -> String {
    method
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && *ch != '/')
        .flat_map(char::to_lowercase)
        .collect()
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(ToOwned::to_owned)
}

fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(Value::as_str) {
                return Some(text.to_string());
            }
            for key in ["content", "delta", "chunk"] {
                if let Some(text) = map.get(key).and_then(extract_text) {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(values) => values.iter().find_map(extract_text),
        Value::String(text) => Some(text.clone()),
        _ => None,
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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpEnvironmentEntry {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for AcpEnvironmentEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpEnvironmentEntry")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpHttpHeader {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for AcpHttpHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpHttpHeader")
            .field("name", &self.name)
            .field("value", &"<redacted>")
            .finish()
    }
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
    fn mcp_servers_serialize_official_stdio_and_sse_shapes() {
        let stdio = McpServer::Stdio(McpServerStdio::new("local", "/bin/echo").args(["hello"]));
        let stdio_value = serde_json::to_value(stdio).unwrap();
        assert_eq!(stdio_value["name"], "local");
        assert_eq!(stdio_value["command"], "/bin/echo");
        assert_eq!(stdio_value["args"][0], "hello");
        assert!(stdio_value.get("type").is_none());

        let sse = McpServer::Sse(
            McpServerSse::new("remote", "https://example.test/sse").headers([AcpHttpHeader {
                name: "Authorization".to_string(),
                value: "Bearer token".to_string(),
            }]),
        );
        let sse_value = serde_json::to_value(sse).unwrap();
        assert_eq!(sse_value["type"], "sse");
        assert_eq!(sse_value["name"], "remote");
        assert_eq!(sse_value["url"], "https://example.test/sse");
        assert_eq!(sse_value["headers"][0]["name"], "Authorization");
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

    #[test]
    fn session_update_round_trips_known_variants() {
        let updates = vec![
            SessionUpdate::AgentMessageChunk {
                text: "hello".to_string(),
            },
            SessionUpdate::AgentThoughtChunk {
                text: "thinking".to_string(),
            },
            SessionUpdate::ToolCall {
                id: "tool-1".to_string(),
                name: "read_file".to_string(),
                args: serde_json::json!({"path":"README.md"}),
            },
            SessionUpdate::ToolCallUpdate {
                id: "tool-1".to_string(),
                status: Some("completed".to_string()),
                args: Some(serde_json::json!({"path":"README.md"})),
                output: Some("done".to_string()),
            },
            SessionUpdate::Plan {
                content: "1. Test".to_string(),
            },
            SessionUpdate::CurrentModeUpdate {
                mode: serde_json::json!({"id":"build"}),
            },
            SessionUpdate::AvailableCommandsUpdate {
                commands: vec![serde_json::json!({"name":"help"})],
            },
        ];

        for update in updates {
            let value = serde_json::to_value(&update).unwrap();
            let reparsed: SessionUpdate = serde_json::from_value(value).unwrap();
            assert_eq!(reparsed, update);
        }
    }

    #[test]
    fn session_update_preserves_unknown_forward_compatible_payload() {
        let value = serde_json::json!({
            "sessionUpdate": "future_event",
            "payload": {"x": 1}
        });

        let update: SessionUpdate = serde_json::from_value(value.clone()).unwrap();

        assert_eq!(
            update,
            SessionUpdate::Unknown {
                method: "future_event".to_string(),
                params: value,
            }
        );
    }

    #[test]
    fn session_update_parses_wrapped_notification_params() {
        let params = serde_json::json!({
            "sessionId": "s1",
            "update": {"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"hello"}}
        });

        assert_eq!(
            SessionUpdate::from_notification_params(&params),
            Some(SessionUpdate::AgentMessageChunk {
                text: "hello".to_string()
            })
        );
    }

    #[test]
    fn mcp_stdio_env_debug_redacts_values() {
        let server = McpServerStdio {
            name: "local".to_string(),
            command: "/bin/echo".into(),
            args: Vec::new(),
            env: vec![AcpEnvironmentEntry {
                name: "TOKEN".to_string(),
                value: "secret-token".to_string(),
            }],
        };

        let debug = format!("{server:?}");
        assert!(debug.contains("TOKEN"));
        assert!(!debug.contains("secret-token"));
        assert!(debug.contains("<redacted>"));
    }
}
