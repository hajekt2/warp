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

Warp advertises capabilities from a per-run snapshot of the active Agent Mode autonomy profile. File reads/writes stay workspace-contained, writes require `apply_code_diffs = AlwaysAllow`, terminal execution requires `execute_commands = AlwaysAllow`, and permission requests only auto-select when an existing profile already grants the corresponding capability.

## Security Boundary

- No `sh -c`, shell expansion, pipes, or redirection in configured agent launch.
- Resolve command paths before launch and report missing executables with registry install URLs when available.
- Do not sync executable command/args/env in a way that silently executes on another device; require local confirmation.
- MCP forwarding is per-agent/per-server opt-in. Allowlist entries can reference explicit stdio argv (`name | command args`) or Warp-installed templatable MCP servers by UUID/name; env/header values remain local and must be redacted from logs, snapshots, and error UI.
- `fs/write_text_file`, `terminal/create`, and `session/request_permission` return only after Warp applies the active approval policy; disallowed requests return JSON-RPC errors or a cancellation-shaped permission response.

## Current Implementation Notes

- `Harness::Acp` is a fieldless coarse slug only. Dynamic per-agent identity lives in settings and UI state, not in `CLIAgent` or a field-bearing `Harness` enum variant.
- `AgentHarnessSelection::{Builtin, Acp}` preserves existing built-in harness behavior while carrying `AcpAgentId` locally.
- `crates/warp_acp` owns local schema-shaped models for initialize/auth/session/list/load/resume/close, fs, terminal, permission, stdio MCP, and capability-gated SSE/HTTP MCP shapes; future work may replace these with `agent-client-protocol-schema` after resolving the repo's MCP dependency constraints.

## Rollout

ACP client support remains guarded by `FeatureFlag::AcpClient`, but `acp_client` is included in the default Cargo feature set for Stable rollout. The flag is intentionally retained for 1–2 release cycles as a rollback lever.

## Verification

- Unit tests for protocol serialization, argv validation, JSON-RPC framing, request/response correlation, notification handling, malformed JSON, timeout, cancellation, and abnormal child exit.
- Settings tests for serialization, feature gating, and local-only ACP config storage.
- UI model/selector tests for built-in and configured ACP entries.
- Integration fixture binary for ACP initialize/session/new/session/prompt echo.
- Manual matrix: `opencode acp --port 0`, `codex-acp`, missing command, crash during handshake, streaming output into a Warp block, MCP allowlist by explicit argv and installed server UUID/name, fs read/write bridge, terminal lifecycle bridge, and session list/load/resume/close protocol calls.
