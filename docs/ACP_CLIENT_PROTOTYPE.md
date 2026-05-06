# Warp ACP Client Prototype

This fork contains an experimental implementation of Warp as an [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) client.

The goal is to let Warp act as a native UI for external, local agent runtimes such as OpenCode or Codex-compatible ACP agents. In this model, the agent process owns model access, provider accounts, API keys, subscriptions, and tool execution, while Warp owns the client-side experience: local process launch, conversation UI, approvals, diffs, terminal integration, and rendered history.

> [!WARNING]
> This is an experimental fork/prototype, not an official Warp release. It is intended for review, testing, and discussion with the Warp open-source community.

## Demo

A short screen recording is included in this repository:

<video src="assets/warp-acp-demo.mp4" controls width="100%"></video>

If your Markdown viewer does not render embedded videos, open [`assets/warp-acp-demo.mp4`](assets/warp-acp-demo.mp4) directly.

## Why this matters

Warp's open-source release makes it possible to explore a more open agent architecture:

- Use Warp as an agentic terminal/ADE frontend.
- Bring your own local or external ACP-compatible agent runtime.
- Let users keep their own model provider subscriptions, API keys, or local model setup inside the agent runtime.
- Avoid coupling Warp's native UI exclusively to one hosted AI backend.
- Preserve Warp's UI strengths while allowing agent/backend choice.

This is closely related to Warp's public roadmap item for ACP support, where ACP is described as a way for other coding agents to use Warp's native agent UX and unlock local model support through harnesses such as OpenCode.

## What works in this prototype

- ACP protocol crate and local stdio runner path.
- Configurable ACP agent entries, including seeded OpenCode/Codex-style commands.
- ACP initialization and session creation.
- Prompt forwarding to an ACP agent.
- Streaming ACP output into Warp's agent conversation/history model.
- Protocol-level smoke tests with OpenCode ACP.
- Protocol-level smoke tests with a Codex ACP wrapper.
- MCP server serialization plumbing with explicit allowlist-oriented design.

For implementation details and test notes, see:

- [`specs/GH7326/PRODUCT.md`](../specs/GH7326/PRODUCT.md)
- [`specs/GH7326/TECH.md`](../specs/GH7326/TECH.md)
- [`specs/GH7326/MANUAL_TESTS.md`](../specs/GH7326/MANUAL_TESTS.md)

## Architecture

```text
Warp UI / Agent UX
        │
        ▼
Warp ACP client
        │  JSON-RPC over local stdio
        ▼
ACP-compatible agent runtime
  ├─ OpenCode ACP
  ├─ Codex ACP wrapper
  └─ other compatible agents
        │
        ▼
User-owned model access
  ├─ provider API keys
  ├─ provider subscriptions
  ├─ local models
  └─ custom tool/MCP setup
```

Warp remains the client. The external agent runtime remains responsible for model selection, billing relationship, credentials, and provider-specific behavior.

## Security and privacy principles

This prototype is intentionally local-first:

- ACP agents are launched as local processes.
- Model credentials should stay with the configured agent runtime, not inside Warp.
- MCP forwarding should be explicit and allowlist-based.
- Warp should not implicitly expose all configured MCP servers to every agent.
- File, terminal, diff, and permission requests should remain gated by Warp's existing autonomy/approval model.

## Running locally

From the repository root:

```bash
./script/bootstrap
./script/run
```

Then configure an ACP-compatible agent command in Warp settings. Example command shape:

```bash
opencode acp --port 0
```

The exact command depends on the agent runtime you want to connect.

## Validation

Useful checks for this area:

```bash
cargo test -p warp_acp
xvfb-run -a env WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS=1 \
  cargo test -p integration test_acp_streaming_exchange_renders_in_agent_history -- --nocapture
```

See [`specs/GH7326/MANUAL_TESTS.md`](../specs/GH7326/MANUAL_TESTS.md) for manual smoke-test notes.

## Status

This branch is best treated as a reviewable prototype. The next useful steps are:

1. Get feedback from the Warp maintainers/community on whether the architecture matches the planned ACP direction.
2. Clean up any prototype-only code paths.
3. Decide whether to upstream as a draft PR, split into smaller PRs, or keep as an experimental fork.

## Relationship to Warp

This repository is a fork of [`warpdotdev/warp`](https://github.com/warpdotdev/warp). It is not affiliated with or endorsed by Warp.

Warp's UI framework crates are MIT licensed; the rest of the codebase is AGPLv3. See the root repository licenses for details.
