use crate::AcpAgentCommand;

/// Seed metadata for well-known ACP-capable agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownAcpAgent {
    pub registry_key: &'static str,
    pub name: &'static str,
    pub command: AcpAgentCommand,
    pub install_url: Option<&'static str>,
    pub notes: &'static str,
}

#[must_use]
pub fn opencode_registry_entry() -> KnownAcpAgent {
    KnownAcpAgent {
        registry_key: "opencode",
        name: "OpenCode",
        command: AcpAgentCommand::new("opencode").args(["acp"]),
        install_url: Some("https://opencode.ai"),
        notes: "Local `opencode acp` is the primary v1 smoke-test target.",
    }
}

#[must_use]
pub fn codex_acp_registry_entry() -> KnownAcpAgent {
    KnownAcpAgent {
        registry_key: "codex-acp",
        name: "Codex ACP wrapper",
        command: AcpAgentCommand::new("codex-acp"),
        install_url: Some("https://www.npmjs.com/package/@zed-industries/codex-acp"),
        notes: "Use the wrapper until the Codex CLI exposes a native ACP command.",
    }
}

#[must_use]
pub fn known_acp_agents() -> Vec<KnownAcpAgent> {
    vec![opencode_registry_entry(), codex_acp_registry_entry()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_entries_are_argv_only() {
        for agent in known_acp_agents() {
            agent.command.validate_argv_only().unwrap();
        }
    }
}
