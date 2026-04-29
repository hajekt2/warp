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
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false},"clientInfo":{"name":"Warp smoke"}}}' \
  | timeout 8 opencode acp
```

Result on 2026-04-29: blocked locally before handshake because OpenCode failed to start its server on port `4096`. The log was written to `~/.local/share/opencode/log/2026-04-29T220517.log`. Re-test after freeing/configuring that port.
