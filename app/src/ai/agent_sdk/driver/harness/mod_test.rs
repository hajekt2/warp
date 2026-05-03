use super::{harness_kind, validate_cli_installed, HarnessKind};
use crate::ai::agent_sdk::driver::AgentDriverError;
use warp_cli::agent::Harness;
use warp_core::features::FeatureFlag;

fn assert_harness_setup_failed(err: &AgentDriverError) -> (&str, &str) {
    match err {
        AgentDriverError::HarnessSetupFailed { harness, reason } => (harness, reason),
        other => panic!("expected HarnessSetupFailed, got: {other}"),
    }
}

#[cfg(not(windows))]
#[test]
fn validate_cli_installed_succeeds_for_known_binary() {
    assert!(validate_cli_installed("ls", None).is_ok());
}

#[test]
fn validate_cli_installed_fails_for_missing_binary() {
    let err = validate_cli_installed("__nonexistent_cli_abc123__", None).unwrap_err();
    let (harness, reason) = assert_harness_setup_failed(&err);
    assert_eq!(harness, "__nonexistent_cli_abc123__");
    assert!(reason.contains("not found"));
    assert!(!reason.contains("Install it first"));
}

#[test]
fn validate_cli_installed_includes_docs_url_in_error() {
    let url = "https://example.com/install";
    let err = validate_cli_installed("__nonexistent_cli_abc123__", Some(url)).unwrap_err();
    let (_, reason) = assert_harness_setup_failed(&err);
    assert!(reason.contains(url));
    assert!(reason.contains("Install it first"));
}

#[test]
fn opencode_legacy_harness_targets_opencode_acp_agent() {
    let _enabled = FeatureFlag::AcpClient.override_enabled(true);

    let HarnessKind::Acp(harness) = harness_kind(Harness::OpenCode).unwrap() else {
        panic!("expected OpenCode legacy harness to route through ACP");
    };

    assert_eq!(harness.agent_id().unwrap().as_str(), "opencode");
}
