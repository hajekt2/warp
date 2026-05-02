# Repository Guidelines

## Project Structure & Module Organization

Warp is a Rust Cargo workspace. The main desktop app and binary live in `app/`; shared libraries live in `crates/` (for example `crates/warpui/`, `crates/graphql/`, `crates/warp_terminal/`, and `crates/warp_completer/`). End-to-end tests use the custom framework in `crates/integration/`. Static assets and bundled resources are under `app/assets/`, `app/resources/`, and `resources/`. Development scripts are in `script/`, and feature/product specs are stored in `specs/`.

## Build, Test, and Development Commands

- `./script/bootstrap` installs platform-specific build prerequisites.
- `cargo run` builds and runs Warp locally.
- `./script/run` is the README quick-start wrapper for building and running.
- `cargo bundle --bin warp` bundles the main app.
- `./script/presubmit` runs the standard pre-PR checks: formatting, Clippy, and tests.
- `cargo nextest run --no-fail-fast --workspace --exclude command-signatures-v2` runs the workspace test suite with nextest.
- `cargo test --doc` runs Rust documentation tests.

For local server development, use `cargo run --features with_local_server`; set `SERVER_ROOT_URL` and `WS_SERVER_URL` when targeting a non-default server port.

## Coding Style & Naming Conventions

Use the Rust toolchain in `rust-toolchain.toml` and keep `cargo fmt` clean. Run `cargo clippy --workspace --all-targets --all-features --tests -- -D warnings` before review. Prefer imports over long path qualifiers, inline format arguments such as `format!("{name}")`, and exhaustive `match` arms instead of `_` when practical. Context parameters (`AppContext`, `ViewContext`, `ModelContext`) should be named `ctx` and usually come last. Remove unused parameters rather than prefixing them with `_`.

## Testing Guidelines

Bug fixes need regression tests; non-trivial logic needs unit tests; user-facing flows should add integration coverage in `crates/integration/` when feasible. Place Rust unit tests in sibling files named `${filename}_tests.rs` or `mod_test.rs`, then include them from the module with `#[cfg(test)]` and `#[path = "..."]`.

## Commit & Pull Request Guidelines

Branch names should be prefixed with your handle, for example `alice/fix-parser`. Commit subjects in this repo are concise, imperative summaries that explain what changed and why, often ending with the PR number after merge. Keep PRs focused on one logical change, branch from `master`, run `./script/presubmit`, and use `.github/pull_request_template.md`. Include testing details, linked issues, screenshots for UI changes, and changelog entries (`CHANGELOG-NEW-FEATURE`, `CHANGELOG-IMPROVEMENT`, or `CHANGELOG-BUG-FIX`) when user-visible.

## Security & Configuration

Do not open public issues for security vulnerabilities; follow `SECURITY.md`. Do not commit local runtime state such as `.omx/`, build artifacts, credentials, or machine-specific configuration.
