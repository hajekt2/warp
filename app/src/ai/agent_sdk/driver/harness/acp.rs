//! ACP runner boundary for configured local Agent Client Protocol agents.
//!
//! ACP is not a terminal CLI harness: it owns a JSON-RPC stdio subprocess and
//! maps protocol events into Warp's native agent UI. Keeping a separate type
//! prevents this path from cloning the terminal-output scraping used by CLI
//! harnesses.

use warp_cli::agent::Harness;

use crate::settings::ai::AcpAgentId;

use super::AgentDriverError;

#[derive(Debug, Default)]
pub(crate) struct AcpHarness {
    agent_id: Option<AcpAgentId>,
}

impl AcpHarness {
    pub(crate) fn new() -> Self {
        Self { agent_id: None }
    }

    pub(crate) fn with_agent_id(agent_id: AcpAgentId) -> Self {
        Self {
            agent_id: Some(agent_id),
        }
    }

    pub(crate) fn agent_id(&self) -> Option<&AcpAgentId> {
        self.agent_id.as_ref()
    }

    pub(crate) fn setup_error(&self) -> AgentDriverError {
        AgentDriverError::HarnessSetupFailed {
            harness: Harness::Acp.to_string(),
            reason: "ACP client support is enabled, but this ACP entry could not be resolved to a configured local agent.".into(),
        }
    }
}
