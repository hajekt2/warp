//! ACP runner boundary for configured local Agent Client Protocol agents.
//!
//! This module is intentionally a skeleton. ACP is not a terminal CLI harness:
//! it will own a JSON-RPC stdio subprocess and map protocol events into Warp's
//! native agent UI. Keeping a separate type prevents future work from cloning
//! the terminal-output scraping path used by CLI harnesses.

use warp_cli::agent::Harness;

use super::AgentDriverError;

#[derive(Debug, Default)]
pub(crate) struct AcpHarness;

impl AcpHarness {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn setup_error(&self) -> AgentDriverError {
        AgentDriverError::HarnessSetupFailed {
            harness: Harness::Acp.to_string(),
            reason: "ACP client support is enabled, but the protocol runner is not wired yet. Configure ACP agents through the ACP settings model and run them through the ACP runner once implemented.".into(),
        }
    }
}
