use command::blocking::Command;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Environment variable configured for an ACP agent subprocess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpEnvironmentVariable {
    pub name: String,
    pub value: String,
}

impl AcpEnvironmentVariable {
    #[must_use]
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Argv-only command configuration for launching a local ACP agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgentCommand {
    pub command: PathBuf,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<AcpEnvironmentVariable>,
}

impl AcpAgentCommand {
    #[must_use]
    pub fn new(command: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn env(mut self, env: impl IntoIterator<Item = AcpEnvironmentVariable>) -> Self {
        self.env = env.into_iter().collect();
        self
    }

    pub fn validate_argv_only(&self) -> Result<(), AcpCommandError> {
        if self.command.as_os_str().is_empty() {
            return Err(AcpCommandError::EmptyCommand);
        }

        if invokes_shell_eval(&self.command, &self.args) {
            return Err(AcpCommandError::ShellEvaluationNotAllowed {
                command: self.command.display().to_string(),
            });
        }

        for env in &self.env {
            if env.name.is_empty() || env.name.contains('=') {
                return Err(AcpCommandError::InvalidEnvironmentName(env.name.clone()));
            }
        }

        Ok(())
    }

    pub fn to_std_command(&self) -> Result<Command, AcpCommandError> {
        self.validate_argv_only()?;

        let mut command = Command::new(&self.command);
        command.args(&self.args);
        for env in &self.env {
            command.env(&env.name, &env.value);
        }
        Ok(command)
    }

    #[must_use]
    pub fn display_argv(&self) -> Vec<String> {
        let mut argv = Vec::with_capacity(1 + self.args.len());
        argv.push(self.command.display().to_string());
        argv.extend(self.args.iter().cloned());
        argv
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AcpCommandError {
    #[error("ACP agent command cannot be empty")]
    EmptyCommand,
    #[error(
        "ACP agent command `{command}` would evaluate a shell string; use argv-only command/args"
    )]
    ShellEvaluationNotAllowed { command: String },
    #[error("ACP agent environment variable name `{0}` is invalid")]
    InvalidEnvironmentName(String),
}

fn invokes_shell_eval(command: &Path, args: &[String]) -> bool {
    let program = command
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    matches!(program.as_str(), "sh" | "bash" | "zsh" | "fish") && args.iter().any(|arg| arg == "-c")
        || matches!(program.as_str(), "cmd" | "cmd.exe")
            && args.iter().any(|arg| arg.eq_ignore_ascii_case("/c"))
        || matches!(
            program.as_str(),
            "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe"
        ) && args
            .iter()
            .any(|arg| matches!(arg.to_ascii_lowercase().as_str(), "-command" | "-c"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_argv_only_command_without_shell() {
        let command = AcpAgentCommand::new("opencode")
            .args(["acp"])
            .env([AcpEnvironmentVariable::new("RUST_LOG", "info")]);

        assert_eq!(
            command.display_argv(),
            vec!["opencode".to_string(), "acp".to_string()]
        );
        assert!(command.to_std_command().is_ok());
    }

    #[test]
    fn rejects_shell_evaluation() {
        let err = AcpAgentCommand::new("sh")
            .args(["-c", "opencode acp"])
            .validate_argv_only()
            .unwrap_err();

        assert_eq!(
            err,
            AcpCommandError::ShellEvaluationNotAllowed {
                command: "sh".to_string()
            }
        );
    }

    #[test]
    fn rejects_invalid_env_name() {
        let err = AcpAgentCommand::new("opencode")
            .env([AcpEnvironmentVariable::new("BAD=NAME", "value")])
            .validate_argv_only()
            .unwrap_err();

        assert_eq!(
            err,
            AcpCommandError::InvalidEnvironmentName("BAD=NAME".to_string())
        );
    }
}
