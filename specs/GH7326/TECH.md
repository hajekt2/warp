# TECH — ACP Client Implementation

## Architecture

ACP is a JSON-RPC-over-stdio protocol, not a terminal TUI harness. The implementation adds a separate protocol runner path:

1. Settings store dynamic `AcpAgentConfig` values keyed by `AcpAgentId`.
2. Warp launches configured agents with argv-only subprocess execution.
3. `crates/warp_acp` owns protocol-shaped serde models, JSON-RPC framing, stdio transport, and a small `AcpClient`.
4. `app/src/ai/agent_sdk/driver/harness/acp.rs` adapts the protocol client into Warp's agent driver without reusing terminal scrollback scraping.
5. ACP agent-to-client requests are routed to existing permission, diff, terminal, and MCP approval policies before Warp responds to the agent.

## Protocol Flow

V1 client flow:

`initialize { protocolVersion, clientCapabilities, clientInfo }` → `InitializeResponse` → optional auth follow-up → `session/new` → `session/prompt` with `session/update` notifications → `PromptResponse`.

Warp initially advertises conservative capabilities. `fs`, `terminal`, and MCP forwarding should remain disabled unless the corresponding approval bridge is fully wired.

## Security Boundary

- No `sh -c`, shell expansion, pipes, or redirection in configured agent launch.
- Resolve command paths before launch and report missing executables with registry install URLs when available.
- Do not sync executable command/args/env in a way that silently executes on another device; require local confirmation.
- MCP forwarding is per-agent/per-server opt-in, with env/secret redaction in logs, snapshots, and error UI.
- `fs/write_text_file`, `terminal/create`, and `session/request_permission` return only after Warp approval or rejection.

## Current Implementation Notes

- `Harness::Acp` is a fieldless coarse slug only. Dynamic per-agent identity lives in settings and UI state, not in `CLIAgent` or a field-bearing `Harness` enum variant.
- `AgentHarnessSelection::{Builtin, Acp}` preserves existing built-in harness behavior while carrying `AcpAgentId` locally.
- `crates/warp_acp` starts with local schema-shaped models; future work may replace these with `agent-client-protocol-schema` after resolving the repo's MCP dependency constraints.

## Verification

- Unit tests for protocol serialization, argv validation, JSON-RPC framing, request/response correlation, notification handling, malformed JSON, timeout, cancellation, and abnormal child exit.
- Settings tests for serialization, feature gating, and local-only ACP config storage.
- UI model/selector tests for built-in and configured ACP entries.
- Integration fixture binary for ACP initialize/session/new/session/prompt echo.
- Manual matrix: `opencode acp`, `codex-acp`, missing command, crash during handshake, MCP allowlist with redaction.
