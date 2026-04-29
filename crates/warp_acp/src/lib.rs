//! Thin ACP client foundation for Warp.
//!
//! This crate intentionally starts as a small boundary layer instead of wiring the
//! full upstream SDK into the app. The 2026-04-29 dependency evaluation found the
//! official `agent-client-protocol-schema` crate to be the safest eventual source
//! for protocol models, but the full `agent-client-protocol` SDK currently pulls a
//! crates.io `rmcp` dependency while Warp uses a pinned fork. Keeping this crate
//! schema-shaped and transport-agnostic lets the driver/UI work proceed without
//! creating an MCP dependency split.
//!
//! The local schema subset mirrors the stable ACP v1 handshake/session/prompt
//! shapes needed by the approved plan. It avoids `deny_unknown_fields` so Warp can
//! remain forward-compatible with non-breaking ACP additions.

mod command;
mod registry;
pub mod schema;

pub use command::{AcpAgentCommand, AcpCommandError, AcpEnvironmentVariable};
pub use registry::{codex_acp_registry_entry, known_acp_agents, opencode_registry_entry, KnownAcpAgent};
pub use schema::{
    conservative_initialize_request, ClientCapabilities, ContentBlock, FileSystemCapabilities,
    Implementation, InitializeRequest, McpServer, McpServerStdio, NewSessionRequest, PromptRequest,
    ProtocolVersion, SessionId, TextContent,
};
