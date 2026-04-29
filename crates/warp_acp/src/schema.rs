use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

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
    pub fn new(client_info: Option<Implementation>, client_capabilities: ClientCapabilities) -> Self {
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
        Some(Implementation::new(client_name.into())),
        ClientCapabilities::conservative(),
    )
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

impl McpServerStdio {
    #[must_use]
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
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
}
