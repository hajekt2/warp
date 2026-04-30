# PRODUCT — Warp as an ACP Client

## Summary

Warp should let users configure local Agent Client Protocol (ACP) agents and run them through Warp's native agent UI. The agent process owns model access and API keys; Warp owns the client side of ACP, local process launch, session UI, approval prompts, diffs, and persisted rendered history.

## User Goals

- Configure a named ACP agent in Warp Settings with an argv-style command, for example `opencode acp --port 0` or `codex-acp`.
- Start that configured agent from the agent harness selector without hand-writing commands.
- See streamed ACP output as Warp agent conversation content rather than raw terminal scrollback.
- Review file writes, terminal commands, and permission requests through Warp's existing approval UX.
- Opt in explicitly before Warp shares MCP server definitions with a configured ACP agent.

## V1 Scope

- Warp acts only as an ACP client.
- Users can configure local ACP agents with stable IDs, names, command, args, env metadata, install URL, registry key, local confirmation state, and MCP allowlist. Settings can seed known agents, add a custom `Name | command arg1 arg2` entry, or update an existing `id | Name | command arg1 | env KEY=value | mcp server-id` entry.
- Commands are local-only and launched with `Command::new(command).args(args)`; Warp never interprets command strings through a shell.
- Warp persists rendered block snapshots for history and the protocol crate exposes ACP `session/list`, `session/load`, `session/resume`, and `session/close` so future UI resume surfaces do not require another protocol boundary change.
- Initial curated registry seeds include OpenCode (`opencode acp --port 0`) and Codex via the Zed `codex-acp` wrapper.

## Deferred UI Surfaces

- Warp as an ACP server and remote HTTP/WebSocket ACP transport remain protocol-crate follow-ups because this feature is a local stdio ACP client.
- Auto-forwarding all MCP servers remains blocked by security review; configured per-agent MCP allowlist entries can forward explicit stdio commands or named/UUID Warp-installed MCP servers. SSE MCP forwarding is sent only when the ACP agent advertises `mcpCapabilities.sse`.
- True rendered-history-to-agent resume needs a dedicated product surface even though protocol `session/load`/`session/resume` calls are now available.

## Success Criteria

- A configured ACP echo fixture can initialize, create a session, receive a prompt, stream updates into a live Warp conversation block, and finish that block when the prompt turn ends.
- Missing/malformed commands fail before launch with actionable UI.
- ACP file/terminal/permission requests are workspace-contained and gated by the active Agent Mode autonomy profile before Warp responds.
- Built-in Oz, Claude, Gemini, and local OpenCode flows continue to behave unchanged when ACP is disabled.
