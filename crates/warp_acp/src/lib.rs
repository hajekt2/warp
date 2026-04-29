//! Agent Client Protocol (ACP) support primitives for Warp.
//!
//! This crate intentionally starts as a small, UI-independent boundary: it owns
//! ACP configuration shapes, registry presets, conservative protocol models,
//! and stdio JSON-RPC framing helpers. The Warp app/driver layer can build on
//! these types without coupling protocol tests to UI state.

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const ACP_PROTOCOL_VERSION: u32 = 1;
pub const INITIALIZE_METHOD: &str = "initialize";
pub const SESSION_NEW_METHOD: &str = "session/new";
pub const SESSION_PROMPT_METHOD: &str = "session/prompt";
pub const SESSION_CANCEL_METHOD: &str = "session/cancel";
pub const SESSION_CLOSE_METHOD: &str = "session/close";
pub const SESSION_UPDATE_METHOD: &str = "session/update";
pub const SESSION_REQUEST_PERMISSION_METHOD: &str = "session/request_permission";
pub const FS_READ_TEXT_FILE_METHOD: &str = "fs/read_text_file";
pub const FS_WRITE_TEXT_FILE_METHOD: &str = "fs/write_text_file";
pub const TERMINAL_CREATE_METHOD: &str = "terminal/create";
pub const TERMINAL_OUTPUT_METHOD: &str = "terminal/output";
pub const TERMINAL_RELEASE_METHOD: &str = "terminal/release";
pub const TERMINAL_WAIT_FOR_EXIT_METHOD: &str = "terminal/wait_for_exit";
pub const TERMINAL_KILL_METHOD: &str = "terminal/kill";

/// Stable local identifier for a configured ACP agent.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[serde(transparent)]
#[schemars(description = "Stable local identifier for a configured ACP agent.")]
pub struct AcpAgentId(pub String);

impl AcpAgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Environment variable supplied to an ACP agent process.
///
/// Values are deliberately plain strings at this layer so callers can choose
/// whether they came from local-only settings, a secret reference, or a literal.
/// Settings/UI code must avoid syncing raw secrets unless an explicit secret
/// storage mechanism is used.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(description = "Environment variable supplied to an ACP agent process.")]
pub struct AcpAgentEnvVar {
    pub name: String,
    pub value: String,
}

/// Per-device launch confirmation for a local ACP agent command.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(description = "Per-device launch confirmation for a local ACP agent command.")]
pub struct AcpLocalConfirmation {
    /// A stable fingerprint of the command/args/env that the user approved.
    pub command_fingerprint: String,
    /// RFC3339 timestamp recorded by the app when the user confirms this device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<String>,
}

/// User-configured local ACP agent.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(description = "User-configured local ACP agent.")]
pub struct AcpAgentConfig {
    pub id: AcpAgentId,
    pub name: String,
    /// Executable name or absolute path. The app must spawn it with argv-only
    /// process creation; this field is never interpreted as a shell snippet.
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<AcpAgentEnvVar>,
    /// MCP server IDs that may be forwarded to this agent after explicit consent.
    #[serde(default)]
    pub mcp_allowlist: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_key: Option<String>,
    /// Local-only confirmation that prevents silently executing synced commands
    /// on a new device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_confirmation: Option<AcpLocalConfirmation>,
}

impl AcpAgentConfig {
    pub fn argv(&self) -> (&std::path::Path, &[String]) {
        (self.command.as_path(), &self.args)
    }

    pub fn validate_launch(&self) -> Result<(), AcpLaunchValidationError> {
        validate_argv_only_command(&self.command, &self.args)
    }
}

/// Known registry preset. These are UI seeds, not implicit permission to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownAcpAgent {
    pub registry_key: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub install_url: &'static str,
    pub notes: &'static str,
}

impl KnownAcpAgent {
    pub fn to_config(&self, id: impl Into<String>) -> AcpAgentConfig {
        AcpAgentConfig {
            id: AcpAgentId::new(id),
            name: self.name.to_string(),
            command: PathBuf::from(self.command),
            args: self.args.iter().map(|arg| (*arg).to_string()).collect(),
            env: vec![],
            mcp_allowlist: vec![],
            install_url: Some(self.install_url.to_string()),
            registry_key: Some(self.registry_key.to_string()),
            local_confirmation: None,
        }
    }
}

pub const OPENCODE_ACP_AGENT: KnownAcpAgent = KnownAcpAgent {
    registry_key: "opencode",
    name: "OpenCode",
    command: "opencode",
    args: &["acp"],
    install_url: "https://opencode.ai/",
    notes: "Runs the locally installed OpenCode ACP server with `opencode acp`.",
};

pub const CODEX_ACP_AGENT: KnownAcpAgent = KnownAcpAgent {
    registry_key: "codex-acp",
    name: "Codex (ACP)",
    command: "codex-acp",
    args: &[],
    install_url: "https://github.com/zed-industries/codex-acp/releases",
    notes: "Prefer an installed codex-acp binary. `npx -y @zed-industries/codex-acp` is a manual fallback that must be shown before first run.",
};

pub const KNOWN_ACP_AGENTS: &[KnownAcpAgent] = &[OPENCODE_ACP_AGENT, CODEX_ACP_AGENT];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AcpLaunchValidationError {
    #[error("ACP agent command cannot be empty")]
    EmptyCommand,
    #[error("ACP agent commands must be argv-only; shell wrapper `{0}` is not allowed")]
    ShellWrapper(String),
    #[error("ACP agent command and args must not contain literal newlines")]
    ContainsNewline,
}

/// Reject obvious shell snippets before the app attempts command resolution.
///
/// This is defense in depth. Launch code must still use
/// `Command::new(command).args(args)` and never `sh -c`.
pub fn validate_argv_only_command(
    command: &std::path::Path,
    args: &[String],
) -> Result<(), AcpLaunchValidationError> {
    if command.as_os_str().is_empty() {
        return Err(AcpLaunchValidationError::EmptyCommand);
    }

    let command_lossy = command.to_string_lossy();
    if command_lossy.contains('\n') || args.iter().any(|arg| arg.contains('\n')) {
        return Err(AcpLaunchValidationError::ContainsNewline);
    }

    let Some(file_name) = command.file_name().and_then(|name| name.to_str()) else {
        return Ok(());
    };
    let normalized = file_name.to_ascii_lowercase();
    let is_shell = matches!(
        normalized.as_str(),
        "sh" | "bash" | "zsh" | "fish" | "pwsh" | "powershell" | "cmd" | "cmd.exe"
    );
    if is_shell && args.iter().any(|arg| arg == "-c" || arg == "/c") {
        return Err(AcpLaunchValidationError::ShellWrapper(file_name.to_string()));
    }

    Ok(())
}

/// Best-effort PATH resolver used to fail before spawning when a command is missing.
pub fn resolve_command_path(command: &std::path::Path) -> Option<PathBuf> {
    if command.components().count() > 1 {
        return is_executable_file(command).then(|| command.to_path_buf());
    }

    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|path| path.join(command))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &std::path::Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

/// JSON-RPC request envelope for ACP calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest<T> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'static str,
    pub params: T,
}

impl<T> JsonRpcRequest<T> {
    pub fn new(id: u64, method: &'static str, params: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method,
            params,
        }
    }
}

/// JSON-RPC notification envelope for ACP updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcNotification<T> {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    pub params: T,
}

impl<T> JsonRpcNotification<T> {
    pub fn new(method: &'static str, params: T) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Conservative ACP client capabilities. Optional method families should stay
/// disabled until the Warp approval bridge implements the whole family.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fs: Option<FileSystemClientCapabilities>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSystemClientCapabilities {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_text_file: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub write_text_file: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: u32,
    pub client_capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

impl InitializeParams {
    pub fn conservative(client_name: impl Into<String>, client_version: Option<String>) -> Self {
        Self {
            protocol_version: ACP_PROTOCOL_VERSION,
            client_capabilities: ClientCapabilities::default(),
            client_info: ClientInfo {
                name: client_name.into(),
                version: client_version,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub protocol_version: u32,
    #[serde(default)]
    pub agent_capabilities: Value,
    #[serde(default)]
    pub agent_info: Value,
    #[serde(default)]
    pub auth_methods: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub transport: McpServerTransport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum McpServerTransport {
    Stdio { command: String, args: Vec<String> },
    Http { url: String },
    Sse { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSessionParams {
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPromptParams {
    pub session_id: String,
    pub prompt: Vec<PromptContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PromptContentBlock {
    Text { text: String },
    ResourceLink { uri: String, name: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCancelParams {
    pub session_id: String,
}

/// Encode a JSON-RPC message for ACP stdio.
///
/// ACP stdio uses one UTF-8 JSON-RPC message followed by `\n`. We always use
/// `serde_json::to_string` to avoid embedded literal newlines from pretty JSON.
pub fn encode_stdio_jsonrpc<T: Serialize>(message: &T) -> serde_json::Result<Vec<u8>> {
    let mut serialized = serde_json::to_string(message)?;
    serialized.push('\n');
    Ok(serialized.into_bytes())
}

/// Decode one ACP stdio frame. Returns `None` for an unterminated line so a
/// caller can keep buffering.
pub fn decode_stdio_jsonrpc_line<T: for<'de> Deserialize<'de>>(
    buffer: &[u8],
) -> serde_json::Result<Option<T>> {
    let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') else {
        return Ok(None);
    };
    serde_json::from_slice(&buffer[..newline]).map(Some)
}

/// Build an argv-only process command from ACP config. This small adapter makes
/// the trust boundary explicit for app launch code.
pub fn build_std_command(config: &AcpAgentConfig) -> Result<std::process::Command, AcpLaunchValidationError> {
    config.validate_launch()?;
    let mut command = std::process::Command::new(&config.command);
    command.args(&config.args);
    for env_var in &config.env {
        command.env(OsString::from(&env_var.name), OsString::from(&env_var.value));
    }
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn conservative_initialize_advertises_no_optional_capabilities() {
        let request = JsonRpcRequest::new(
            1,
            INITIALIZE_METHOD,
            InitializeParams::conservative("Warp", Some("0.1.0".to_string())),
        );

        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["method"], INITIALIZE_METHOD);
        assert_eq!(value["params"]["protocolVersion"], ACP_PROTOCOL_VERSION);
        assert!(value["params"]["clientCapabilities"].as_object().unwrap().is_empty());
    }

    #[test]
    fn stdio_encoding_has_one_trailing_delimiter_and_escapes_newlines() {
        let request = JsonRpcRequest::new(
            7,
            SESSION_PROMPT_METHOD,
            SessionPromptParams {
                session_id: "session-1".to_string(),
                prompt: vec![PromptContentBlock::Text {
                    text: "hello\nworld".to_string(),
                }],
            },
        );

        let encoded = encode_stdio_jsonrpc(&request).unwrap();
        let encoded = String::from_utf8(encoded).unwrap();
        assert!(encoded.ends_with('\n'));
        assert_eq!(encoded.matches('\n').count(), 1);
        assert!(encoded.contains("hello\\nworld"));
    }

    #[test]
    fn stdio_decoding_waits_for_complete_line() {
        assert!(decode_stdio_jsonrpc_line::<Value>(br#"{"jsonrpc":"2.0"}"#)
            .unwrap()
            .is_none());

        let decoded = decode_stdio_jsonrpc_line::<Value>(br#"{"jsonrpc":"2.0"}
extra"#)
        .unwrap()
        .unwrap();
        assert_eq!(decoded["jsonrpc"], "2.0");
    }

    #[test]
    fn launch_validation_rejects_shell_interpolation() {
        let err = validate_argv_only_command(
            std::path::Path::new("sh"),
            &["-c".to_string(), "opencode acp".to_string()],
        )
        .unwrap_err();
        assert!(matches!(err, AcpLaunchValidationError::ShellWrapper(_)));
    }

    #[test]
    fn registry_configs_use_argv_not_shell_strings() {
        let opencode = OPENCODE_ACP_AGENT.to_config("opencode-default");
        assert_eq!(opencode.command, PathBuf::from("opencode"));
        assert_eq!(opencode.args, vec!["acp"]);
        opencode.validate_launch().unwrap();

        let codex = CODEX_ACP_AGENT.to_config("codex-default");
        assert_eq!(codex.command, PathBuf::from("codex-acp"));
        assert!(codex.args.is_empty());
        codex.validate_launch().unwrap();
    }
}
