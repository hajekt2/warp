# ACP Rust dependency evaluation

Date: 2026-04-29
Task: ACP protocol/SDK evaluation and crate foundation

## Sources checked

- Official ACP Rust library page: `agent-client-protocol` is the canonical Rust SDK and is used by Zed.
- docs.rs for `agent-client-protocol` 0.11.1: exposes client/agent builders and depends on `agent-client-protocol-schema = 0.12.0` plus `rmcp ^1.2.0`.
- docs.rs for `agent-client-protocol-schema` 0.12.2: provides transport-agnostic serde models for ACP v1 handshake, sessions, prompts, fs, terminal, MCP, permissions, and streaming updates.

## Decision

Do not wire the full `agent-client-protocol` SDK into Warp in this first crate slice. Add a narrow `warp_acp` crate with schema-shaped local v1 models, argv-only subprocess command validation, and known-agent seed metadata.

## Rationale

- The approved plan requires protocol correctness, conservative capabilities, argv-only spawning, and preserving current Oz/Claude/Gemini terminal harness behavior.
- The full SDK is promising for a future transport adapter, but it brings a crates.io `rmcp` dependency while Warp currently uses a pinned `warpdotdev/rmcp` fork. Pulling both in this slice would make the MCP/security boundary harder to review.
- The official schema crate is acceptable as the eventual source of generated/stable protocol models, but this worker environment does not have Cargo installed, so dependency and lockfile updates could not be generated or compiled safely here.
- The local model subset intentionally mirrors stable ACP v1 shapes and remains forward-compatible by not denying unknown fields.

## Follow-up

When Cargo verification is available, replace or back the local `schema` module with `agent-client-protocol-schema` and evaluate whether the full SDK can be adopted after resolving the `rmcp` dependency split.
