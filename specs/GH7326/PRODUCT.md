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
- Users can configure local ACP agents with stable IDs, names, command, args, env metadata, install URL, registry key, local confirmation state, and MCP allowlist.
- Commands are local-only and launched with `Command::new(command).args(args)`; Warp never interprets command strings through a shell.
- Warp persists rendered block snapshots for history. Agent-side session load/resume is out of v1.
- Initial curated registry seeds include OpenCode (`opencode acp --port 0`) and Codex via the Zed `codex-acp` wrapper.

## Out of Scope

- Warp as an ACP server.
- Remote HTTP/WebSocket ACP transport.
- ACP `session/load`, `session/list`, and true agent-side resume.
- Auto-forwarding all MCP servers or synced execution of unconfirmed commands on another device.

## Success Criteria

- A configured ACP echo fixture can initialize, create a session, receive a prompt, stream an update, and finish with a rendered Warp conversation block.
- Missing/malformed commands fail before launch with actionable UI.
- ACP file/terminal/permission requests cannot bypass Warp review.
- Built-in Oz, Claude, Gemini, and local OpenCode flows continue to behave unchanged when ACP is disabled.
