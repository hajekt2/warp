# MANUAL TESTS — ACP Client

## Automated fixture

- `cargo test -p warp_acp` builds and runs `echo_acp_agent`, a deterministic stdio ACP fixture.
- Coverage: `initialize`, `session/new`, `session/prompt`, one `session/update` notification, final `end_turn` response, schema serialization for `session/load`/permission outcomes, and the local fs/terminal request handler.
- 2026-04-30 final verification: passed with
  `BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include" cargo test -p warp_acp`
  (`29` unit tests, `echo_fixture_completes_initialize_session_and_prompt`, and doctests).

## Local ACP CLI smoke checks

Run from `/home/haja/work/warp`.

### Codex ACP wrapper

Command:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"Warp smoke"}}}' \
  | timeout 20 npx -y @zed-industries/codex-acp
```

Result on 2026-04-29 and rechecked on 2026-04-30: passed handshake. The wrapper returned protocol version `1`, `codex-acp` agent info, auth methods for ChatGPT/CODEX_API_KEY/OPENAI_API_KEY, and then stayed alive as expected until `timeout` ended the process.

Direct live prompt smoke on 2026-04-30:

- Command: spawned `npx -y @zed-industries/codex-acp` over stdio, sent `initialize`, `session/new`, then
  `session/prompt` with text `Reply with exactly: WARP_CODEX_ACP_SMOKE_OK`.
- Result: passed. `agentInfo.name = "codex-acp"`, `agentInfo.version = "0.12.0"`,
  `session/new` returned config options, streamed `agent_message_chunk` output
  `WARP_CODEX_ACP_SMOKE_OK`, and `session/prompt` returned `stopReason = "end_turn"`.
  The wrapper also streamed a non-fatal model metadata warning before the requested output.

### OpenCode ACP

Command:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"Warp smoke","version":"0.1.0"}}}' \
  | timeout 8 opencode acp --port 0 --pure
```

Result on 2026-04-29 and rechecked on 2026-04-30: passed handshake with OpenCode `1.14.29`. OpenCode returned protocol version `1`, `agentInfo.name = "OpenCode"`, `agentInfo.version = "1.14.29"`, and auth instructions. Two local compatibility findings were captured:

- Bare `opencode acp` failed before handshake because a long-running orphaned `/home/haja/.npm-global/lib/node_modules/opencode-ai/bin/.opencode serve` process was already listening on `0.0.0.0:4096` (PID `834381`, parent `1`). The Warp registry seeds OpenCode as `opencode acp --port 0` so ACP gets an ephemeral HTTP port and avoids collisions with existing OpenCode servers.
- OpenCode rejects `initialize` when `clientInfo.version` is omitted (`-32602 Invalid params`). Warp's conservative initialize request includes the `warp_acp` crate version for ACP schema compatibility.

Direct live prompt smoke on 2026-04-30:

- Command: spawned `opencode acp --port 0 --pure` over stdio, sent `initialize`, `session/new` with
  `mcpServers: []`, then `session/prompt` with text `Reply with exactly: WARP_ACP_SMOKE_OK`.
- Result: passed. `agentInfo.name = "OpenCode"`, `agentInfo.version = "1.14.29"`,
  `session/new` returned `sessionId = ses_222d7fd28ffeZBTJ3lmqG6lB17`, streamed
  `agent_thought_chunk` and `agent_message_chunk` updates, emitted final text
  `WARP_ACP_SMOKE_OK`, and `session/prompt` returned `stopReason = "end_turn"`.

## Warp bridge checks

Automated unit coverage now exercises:

- workspace-contained `fs/read_text_file` with line windows;
- rejection of file paths outside the workspace;
- workspace-contained `fs/write_text_file` when the policy enables writes;
- ACP `session/list`, `session/load`, `session/resume`, and `session/close` client requests;
- child-process shutdown through transport drop;
- live ACP update plumbing from `session/update` notifications into a streaming Warp conversation exchange;
- official ACP MCP server serialization for untagged stdio and tagged SSE entries.

Run:

```bash
BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include" cargo test -p warp_acp
BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include" cargo check -p warp
BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include" cargo check -p warp --features acp_client
BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include" cargo test -p warp acp --features acp_client
BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include" cargo test -p warp acp_mcp --features acp_client
```

2026-04-30 final verification:

- `cargo check -p warp`: passed.
- `cargo check -p warp --features acp_client`: passed.
- `cargo test -p warp acp --features acp_client`: passed (`10` tests).
- `cargo test -p warp acp_mcp --features acp_client`: passed (`3` tests).

MCP forwarding evidence:

- Explicit `name | command args` is covered by
  `acp_mcp_explicit_stdio_allowlist_entry_parses_as_argv`.
- Bare names and `*` are reserved for installed-server lookup and are not parsed as implicit commands, covered by
  `acp_mcp_bare_allowlist_entry_is_reserved_for_installed_servers`.
- Non-stdio MCP transports are filtered by advertised agent capabilities, covered by
  `acp_mcp_capability_filter_gates_non_stdio_transports`.
- Live OpenCode smoke sent `mcpServers: []` and confirmed no MCP server forwarding is implicit for a new ACP session.

## Integration coverage

`test_acp_streaming_exchange_renders_in_agent_history` is registered in the Warp integration suite. It injects an ACP-style streaming exchange through `BlocklistAIHistoryModel::start/update/finish_acp_streaming_exchange` and asserts that the latest agent-history exchange renders the final streamed text.

2026-04-30 final verification:

- Toolchain unblock: this machine has `libclang1-18` but no `clang` resource headers. Since `sudo` is unavailable, the working local fix is to point bindgen at GCC's resource headers:
  `BINDGEN_EXTRA_CLANG_ARGS="-isystem /usr/lib/gcc/x86_64-linux-gnu/13/include"`.
- `cargo check -p integration`: passed with that environment variable.
- `cargo test -p integration test_acp_streaming_exchange_renders_in_agent_history -- --nocapture`: compiled and launched the registered test, but local execution is blocked by the host environment having no display server (`DISPLAY`/`WAYLAND_DISPLAY` unset, no `Xvfb` installed). The failure occurs while creating the winit event loop:
  `should be able to create event loop: NotSupported(NotSupportedError)`.
  This is an environment/display blocker, not an ACP compile or assertion failure.

## UI/manual limitations in this environment

- Full Warp UI selection tests for OpenCode/Codex ACP, approval dialogs, and visible MCP forwarding controls require a desktop display. This container has no `DISPLAY`, no `WAYLAND_DISPLAY`, and no `Xvfb`, so real Warp window QA could not be completed locally.
- Protocol-level live-agent QA was completed over stdio for OpenCode and Codex ACP as documented above.
- Approval-flow behavior is covered by `cargo test -p warp_acp`: workspace-contained file read, outside-workspace denial, enabled workspace file write, permission cancellation/selection schema, disabled filesystem request rejection before response, terminal command execution and output reporting through `LocalClientRequestHandler`.
