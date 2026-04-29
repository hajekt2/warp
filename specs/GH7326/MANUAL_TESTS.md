# MANUAL TESTS — ACP Client

## Automated fixture

- `cargo test -p warp_acp` builds and runs `echo_acp_agent`, a deterministic stdio ACP fixture.
- Coverage: `initialize`, `session/new`, `session/prompt`, one `session/update` notification, and final `end_turn` response.

## Local ACP CLI smoke checks

Run from `/home/haja/work/warp`.

### Codex ACP wrapper

Command:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"Warp smoke"}}}' \
  | timeout 20 npx -y @zed-industries/codex-acp
```

Result on 2026-04-29: passed handshake. The wrapper returned protocol version `1`, `codex-acp` agent info, auth methods for ChatGPT/CODEX_API_KEY/OPENAI_API_KEY, and then stayed alive as expected until `timeout` ended the process.

### OpenCode ACP

Command:

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"Warp smoke","version":"0.1.0"}}}' \
  | timeout 8 opencode acp --port 0 --pure
```

Result on 2026-04-29: passed handshake with OpenCode `1.14.29`. OpenCode returned protocol version `1`, `agentInfo.name = "OpenCode"`, `agentInfo.version = "1.14.29"`, and auth instructions. Two local compatibility findings were captured:

- Bare `opencode acp` failed before handshake because a long-running orphaned `/home/haja/.npm-global/lib/node_modules/opencode-ai/bin/.opencode serve` process was already listening on `0.0.0.0:4096` (PID `834381`, parent `1`). The Warp registry seeds OpenCode as `opencode acp --port 0` so ACP gets an ephemeral HTTP port and avoids collisions with existing OpenCode servers.
- OpenCode rejects `initialize` when `clientInfo.version` is omitted (`-32602 Invalid params`). Warp's conservative initialize request includes the `warp_acp` crate version for ACP schema compatibility.
