use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
};

use anyhow::{anyhow, Context as _};
use warp_acp::{
    AcpAgentCommand, AcpClient, AgentMessage, AuthenticateRequest, ClientCapabilities,
    ClientRequestUi, ContentBlock, FileSystemCapabilities, Implementation, InitializeRequest,
    LocalClientRequestError, LocalClientRequestHandler, LocalClientRequestPolicy, McpServer,
    NewSessionRequest, RequestPermissionRequest, RequestPermissionResponse,
    SessionId as AcpSessionId, WriteTextFileRequest,
};
use warpui::{
    modals::{AlertDialogWithCallbacks, ModalButton},
    EntityId, ModelDropped, ModelSpawner, SingletonEntity,
};

use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent::AIAgentOutput;
use crate::ai::agent_sdk::driver::{AcpStreamingOutputBuilder, AgentDriver};
use crate::ai::blocklist::history_model::StreamingExchangeHandle;
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::settings::ai::AcpAgentId;

use super::model::AmbientAgentViewModel;

#[derive(Clone)]
pub(super) struct LocalAcpClientRequestUi {
    foreground: ModelSpawner<AmbientAgentViewModel>,
    prompt_for_read: bool,
    prompt_for_write: bool,
    prompt_for_terminal: bool,
}

impl LocalAcpClientRequestUi {
    pub(super) fn new(
        foreground: ModelSpawner<AmbientAgentViewModel>,
        prompt_for_read: bool,
        prompt_for_write: bool,
        prompt_for_terminal: bool,
    ) -> Self {
        Self {
            foreground,
            prompt_for_read,
            prompt_for_write,
            prompt_for_terminal,
        }
    }

    fn request_bool(
        &self,
        title: String,
        body: String,
        approve_label: &'static str,
    ) -> Result<bool, LocalClientRequestError> {
        let (tx, rx) = mpsc::channel();
        let approve_tx = tx.clone();
        let deny_tx = tx;
        let foreground = self.foreground.clone();
        futures::executor::block_on(foreground.spawn(move |_, ctx| {
            ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_app(
                title,
                body,
                vec![
                    ModalButton::for_app(approve_label, move |_| {
                        let _ = approve_tx.send(true);
                    }),
                    ModalButton::for_app("Deny", move |_| {
                        let _ = deny_tx.send(false);
                    }),
                ],
                |_| {},
            ));
        }))
        .map_err(|_| local_ui_error("ACP approval UI is unavailable"))?;
        rx.recv()
            .map_err(|_| local_ui_error("ACP approval UI was dismissed before a decision"))
    }
}

impl ClientRequestUi for LocalAcpClientRequestUi {
    fn approve_read_text_file(
        &self,
        request: &warp_acp::ReadTextFileRequest,
        resolved_path: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        if !self.prompt_for_read {
            return Ok(true);
        }
        self.request_bool(
            "Allow ACP agent to read a file?".to_string(),
            format!(
                "The local ACP agent requested read access to:\n{}\n\nOriginal path: {}",
                resolved_path.display(),
                request.path.display()
            ),
            "Allow Read",
        )
    }

    fn approve_write_text_file(
        &self,
        request: &WriteTextFileRequest,
        resolved_path: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        if !self.prompt_for_write {
            return Ok(true);
        }
        self.request_bool(
            "Allow ACP agent to write a file?".to_string(),
            format!(
                "The local ACP agent requested write access to:\n{}\n\nOriginal path: {}\nNew content size: {} bytes",
                resolved_path.display(),
                request.path.display(),
                request.content.len()
            ),
            "Allow Write",
        )
    }

    fn request_permission(
        &self,
        request: &RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, LocalClientRequestError> {
        if request.options.is_empty() {
            return Ok(RequestPermissionResponse::cancelled());
        }

        let (tx, rx) = mpsc::channel();
        let options = request.options.clone();
        let tool_call = serde_json::to_string_pretty(&request.tool_call)
            .unwrap_or_else(|_| request.tool_call.to_string());
        let foreground = self.foreground.clone();
        futures::executor::block_on(foreground.spawn(move |_, ctx| {
            let mut buttons = Vec::with_capacity(options.len() + 1);
            for option in options {
                let option_tx = tx.clone();
                let label = option.name.clone();
                let option_id = option.option_id.clone();
                buttons.push(ModalButton::for_app(label, move |_| {
                    let _ = option_tx.send(RequestPermissionResponse {
                        outcome: warp_acp::RequestPermissionOutcome::Selected { option_id },
                    });
                }));
            }
            buttons.push(ModalButton::for_app("Cancel", move |_| {
                let _ = tx.send(RequestPermissionResponse::cancelled());
            }));
            ctx.show_native_platform_modal(AlertDialogWithCallbacks::for_app(
                "ACP agent requests permission",
                format!(
                    "Tool call:
{tool_call}"
                ),
                buttons,
                |_| {},
            ));
        }))
        .map_err(|_| local_ui_error("ACP permission UI is unavailable"))?;
        rx.recv()
            .map_err(|_| local_ui_error("ACP permission UI was dismissed before a decision"))
    }

    fn approve_terminal(
        &self,
        request: &warp_acp::CreateTerminalRequest,
        resolved_cwd: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        if !self.prompt_for_terminal {
            return Ok(true);
        }
        self.request_bool(
            "Allow ACP agent to run a terminal command?".to_string(),
            format!(
                "Command: {} {}\nWorking directory: {}",
                request.command,
                request.args.join(" "),
                resolved_cwd.display()
            ),
            "Allow Command",
        )
    }
}

fn local_ui_error(message: &'static str) -> LocalClientRequestError {
    LocalClientRequestError::Io(std::io::Error::new(std::io::ErrorKind::BrokenPipe, message))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LocalAcpAgentStatus {
    Idle,
    Prompting,
    Failed { error_message: String },
}

#[derive(Clone)]
pub(super) struct LocalAcpRuntimeSession {
    agent_id: AcpAgentId,
    command_argv: Vec<String>,
    working_dir: PathBuf,
    request_policy: LocalClientRequestPolicy,
    client: Arc<AcpClient>,
    session_id: AcpSessionId,
}

impl LocalAcpRuntimeSession {
    fn matches(
        &self,
        agent_id: &AcpAgentId,
        command_argv: &[String],
        working_dir: &PathBuf,
        request_policy: &LocalClientRequestPolicy,
    ) -> bool {
        &self.agent_id == agent_id
            && self.command_argv == command_argv
            && &self.working_dir == working_dir
            && &self.request_policy == request_policy
    }
}

pub(super) struct LocalAcpAgentModel {
    status: LocalAcpAgentStatus,
    runtime_session: Option<LocalAcpRuntimeSession>,
    conversation_id: Option<AIConversationId>,
}

impl LocalAcpAgentModel {
    pub(super) fn new() -> Self {
        Self {
            status: LocalAcpAgentStatus::Idle,
            runtime_session: None,
            conversation_id: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.status = LocalAcpAgentStatus::Idle;
        self.runtime_session = None;
        self.conversation_id = None;
    }

    pub(super) fn conversation_id(&self) -> Option<AIConversationId> {
        self.conversation_id
    }

    pub(super) fn set_conversation_id(&mut self, id: Option<AIConversationId>) {
        self.conversation_id = id;
    }

    pub(super) fn matching_session(
        &self,
        agent_id: &AcpAgentId,
        command_argv: &[String],
        working_dir: &PathBuf,
        request_policy: &LocalClientRequestPolicy,
    ) -> Option<LocalAcpRuntimeSession> {
        self.runtime_session
            .as_ref()
            .filter(|session| session.matches(agent_id, command_argv, working_dir, request_policy))
            .cloned()
    }

    pub(super) fn mark_prompting(&mut self) {
        self.status = LocalAcpAgentStatus::Prompting;
    }

    pub(super) fn mark_finished(
        &mut self,
        conversation_id: AIConversationId,
        runtime_session: LocalAcpRuntimeSession,
    ) {
        self.status = LocalAcpAgentStatus::Idle;
        self.conversation_id = Some(conversation_id);
        self.runtime_session = Some(runtime_session);
    }

    pub(super) fn mark_failed(&mut self, error_message: String) {
        self.status = LocalAcpAgentStatus::Failed { error_message };
        self.runtime_session = None;
    }
}

pub(super) struct LocalAcpPromptRequest {
    pub(super) agent_id: AcpAgentId,
    pub(super) command: AcpAgentCommand,
    pub(super) command_argv: Vec<String>,
    pub(super) prompt: String,
    pub(super) working_dir: PathBuf,
    pub(super) request_policy: LocalClientRequestPolicy,
    pub(super) mcp_servers: Vec<McpServer>,
    pub(super) terminal_view_id: EntityId,
    pub(super) existing_conversation_id: Option<AIConversationId>,
    existing_session: Option<LocalAcpRuntimeSession>,
    pub(super) foreground: ModelSpawner<AmbientAgentViewModel>,
    pub(super) client_request_ui: Arc<dyn ClientRequestUi>,
}

impl LocalAcpPromptRequest {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        agent_id: AcpAgentId,
        command: AcpAgentCommand,
        command_argv: Vec<String>,
        prompt: String,
        working_dir: PathBuf,
        request_policy: LocalClientRequestPolicy,
        mcp_servers: Vec<McpServer>,
        terminal_view_id: EntityId,
        existing_conversation_id: Option<AIConversationId>,
        existing_session: Option<LocalAcpRuntimeSession>,
        foreground: ModelSpawner<AmbientAgentViewModel>,
        client_request_ui: Arc<dyn ClientRequestUi>,
    ) -> Self {
        Self {
            agent_id,
            command,
            command_argv,
            prompt,
            working_dir,
            request_policy,
            mcp_servers,
            terminal_view_id,
            existing_conversation_id,
            existing_session,
            foreground,
            client_request_ui,
        }
    }

    pub(super) async fn run(self) -> anyhow::Result<LocalAcpPromptResult> {
        log::info!(
            "Starting local ACP agent '{}' with argv {:?}",
            self.agent_id.as_str(),
            self.command_argv
        );

        let stream_handle = self.start_history_exchange().await?;
        log::info!(
            "Local ACP agent '{}' created history conversation {}",
            self.agent_id.as_str(),
            stream_handle.conversation_id
        );
        self.emit_conversation_ready(stream_handle.conversation_id)
            .await?;

        let (output_tx, mut output_rx) = tokio::sync::mpsc::unbounded_channel::<AIAgentOutput>();
        let acp_run = tokio::task::spawn_blocking({
            let agent_id = self.agent_id.clone();
            let command = self.command.clone();
            let command_argv = self.command_argv.clone();
            let prompt = self.prompt.clone();
            let working_dir = self.working_dir.clone();
            let request_policy = self.request_policy.clone();
            let mcp_servers = self.mcp_servers.clone();
            let existing_session = self.existing_session.clone();
            let client_request_ui = Arc::clone(&self.client_request_ui);
            move || {
                run_prompt_blocking(
                    agent_id,
                    command,
                    command_argv,
                    prompt,
                    working_dir,
                    request_policy,
                    mcp_servers,
                    existing_session,
                    output_tx,
                    client_request_ui,
                )
            }
        });

        tokio::pin!(acp_run);
        let mut run_error = None;
        let mut completed_acp_session = None;
        let output_text = loop {
            tokio::select! {
                maybe_output = output_rx.recv() => {
                    if let Some(output) = maybe_output {
                        self.update_history_output(&stream_handle, output).await?;
                    }
                }
                result = &mut acp_run => {
                    match result.map_err(|error| anyhow!(error))? {
                        Ok((output, acp_session)) => {
                            completed_acp_session = Some(acp_session);
                            break output;
                        }
                        Err(error) => {
                            let error_text = format!("{error:#}");
                            log::error!(
                                "Local ACP agent '{}' failed: {}",
                                self.agent_id.as_str(),
                                error_text
                            );
                            run_error = Some(anyhow!(error_text.clone()));
                            break AcpStreamingOutputBuilder::error_output(format!(
                                "ACP agent failed: {error_text}"
                            ));
                        }
                    }
                }
            }
        };

        while let Ok(output) = output_rx.try_recv() {
            self.update_history_output(&stream_handle, output).await?;
        }

        let final_output = if output_text.messages.is_empty() {
            AcpStreamingOutputBuilder::error_output(
                "ACP agent completed without streamed text output.",
            )
        } else {
            output_text
        };
        self.finish_history_exchange(&stream_handle, final_output)
            .await?;

        if let Some(error) = run_error {
            log::warn!(
                "Local ACP agent '{}' finished with visible conversation error: {error:#}",
                self.agent_id.as_str()
            );
        }
        let Some(runtime_session) = completed_acp_session else {
            return Err(anyhow!("Local ACP prompt failed"));
        };
        Ok(LocalAcpPromptResult {
            conversation_id: stream_handle.conversation_id,
            runtime_session,
        })
    }

    async fn start_history_exchange(&self) -> anyhow::Result<StreamingExchangeHandle> {
        let terminal_view_id = self.terminal_view_id;
        let existing_conversation_id = self.existing_conversation_id;
        let prompt_for_history = self.prompt.clone();
        let working_dir_for_history = self.working_dir.clone();
        self.foreground
            .spawn(move |_, ctx| {
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    history.start_streaming_exchange_in_conversation(
                        terminal_view_id,
                        existing_conversation_id,
                        prompt_for_history,
                        Some(working_dir_for_history.display().to_string()),
                        ctx,
                    )
                })
            })
            .await?
            .map_err(|error| anyhow!(error))
    }

    async fn emit_conversation_ready(
        &self,
        conversation_id: AIConversationId,
    ) -> Result<(), ModelDropped> {
        self.foreground
            .spawn(move |_, ctx| {
                ctx.emit(
                    super::model::AmbientAgentViewModelEvent::LocalAcpConversationReady {
                        conversation_id,
                    },
                );
            })
            .await
    }

    async fn update_history_output(
        &self,
        stream_handle: &StreamingExchangeHandle,
        output: AIAgentOutput,
    ) -> anyhow::Result<()> {
        let terminal_view_id = self.terminal_view_id;
        let handle = stream_handle.clone();
        self.foreground
            .spawn(move |_, ctx| {
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    history.update_streaming_exchange_output(terminal_view_id, &handle, output, ctx)
                })
            })
            .await?
            .map_err(|error| anyhow!(error))
    }

    async fn finish_history_exchange(
        &self,
        stream_handle: &StreamingExchangeHandle,
        final_output: AIAgentOutput,
    ) -> anyhow::Result<()> {
        let terminal_view_id = self.terminal_view_id;
        let handle = stream_handle.clone();
        self.foreground
            .spawn(move |_, ctx| {
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    history.update_streaming_exchange_output(
                        terminal_view_id,
                        &handle,
                        final_output,
                        ctx,
                    )?;
                    history.finish_streaming_exchange_current_output(terminal_view_id, &handle, ctx)
                })
            })
            .await?
            .map_err(|error| anyhow!(error))
    }
}

pub(super) struct LocalAcpPromptResult {
    pub(super) conversation_id: AIConversationId,
    runtime_session: LocalAcpRuntimeSession,
}

impl LocalAcpPromptResult {
    pub(super) fn into_runtime_session(self) -> LocalAcpRuntimeSession {
        self.runtime_session
    }
}

fn run_prompt_blocking(
    agent_id: AcpAgentId,
    command: AcpAgentCommand,
    command_argv: Vec<String>,
    prompt: String,
    working_dir: PathBuf,
    request_policy: LocalClientRequestPolicy,
    mcp_servers: Vec<McpServer>,
    existing_session: Option<LocalAcpRuntimeSession>,
    output_tx: tokio::sync::mpsc::UnboundedSender<AIAgentOutput>,
    client_request_ui: Arc<dyn ClientRequestUi>,
) -> anyhow::Result<(AIAgentOutput, LocalAcpRuntimeSession)> {
    let acp_session = if let Some(session) = existing_session {
        log::info!(
            "Reusing local ACP session {:?} for agent '{}'",
            session.session_id,
            agent_id.as_str()
        );
        session
    } else {
        create_runtime_session(
            agent_id,
            command,
            command_argv,
            working_dir,
            request_policy,
            mcp_servers,
        )?
    };

    let mut request_handler = LocalClientRequestHandler::new(acp_session.request_policy.clone())
        .map_err(|error| anyhow!(error))?
        .with_ui(client_request_ui);
    let output = Arc::new(Mutex::new(AcpStreamingOutputBuilder::default()));
    let output_for_notifications = Arc::clone(&output);
    acp_session
        .client
        .prompt_with_agent_message_and_request_handler(
            acp_session.session_id.clone(),
            vec![ContentBlock::text(prompt)],
            move |message: AgentMessage| {
                if let Some(update) = AgentDriver::acp_session_update(&message) {
                    let mut builder = output_for_notifications
                        .lock()
                        .expect("ACP output builder poisoned");
                    if builder.apply_update(update) {
                        let _ = output_tx.send(builder.output());
                    }
                } else {
                    log::debug!("Ignoring non-session ACP agent message: {message:?}");
                }
            },
            move |message, transport| request_handler.handle(message, transport),
        )
        .map_err(anyhow::Error::from)?;
    log::info!("Local ACP prompt completed");
    let output = output.lock().expect("ACP output builder poisoned").output();
    Ok((output, acp_session))
}

fn create_runtime_session(
    agent_id: AcpAgentId,
    command: AcpAgentCommand,
    command_argv: Vec<String>,
    working_dir: PathBuf,
    request_policy: LocalClientRequestPolicy,
    mcp_servers: Vec<McpServer>,
) -> anyhow::Result<LocalAcpRuntimeSession> {
    log::info!("Local ACP spawning subprocess");
    let client = AcpClient::spawn(&command).map_err(anyhow::Error::from)?;
    log::info!("Local ACP subprocess spawned; initializing");
    let fs_capabilities = match (
        request_policy.allow_read_text_file,
        request_policy.allow_write_text_file,
    ) {
        (true, true) => FileSystemCapabilities::read_write(),
        (true, false) => FileSystemCapabilities::read_only(),
        _ => FileSystemCapabilities::none(),
    };
    let initialize_response = client
        .initialize_with(InitializeRequest::new(
            Some(Implementation::new("Warp").with_version(env!("CARGO_PKG_VERSION"))),
            ClientCapabilities::conservative()
                .with_file_system(fs_capabilities)
                .with_terminal(request_policy.allow_terminal),
        ))
        .map_err(anyhow::Error::from)?;
    log::info!(
        "Local ACP initialized agent {:?} with {} auth method(s)",
        initialize_response.agent_info,
        initialize_response.auth_methods.len()
    );
    if let Some(method_id) =
        AgentDriver::preferred_acp_auth_method(&initialize_response.auth_methods)
    {
        log::info!("Local ACP authenticating with method `{method_id}`");
        client
            .authenticate(AuthenticateRequest::new(method_id.clone()))
            .with_context(|| format!("ACP authentication failed for method `{method_id}`"))?;
    }
    let mcp_servers = AgentDriver::filter_acp_mcp_servers_for_agent_capabilities(
        mcp_servers,
        &initialize_response.agent_capabilities,
    );
    let session = client
        .new_session(NewSessionRequest::new(working_dir.clone()).with_mcp_servers(mcp_servers))
        .map_err(anyhow::Error::from)?;
    log::info!("Local ACP session created: {:?}", session.session_id);
    Ok(LocalAcpRuntimeSession {
        agent_id,
        command_argv,
        working_dir,
        request_policy,
        client: Arc::new(client),
        session_id: session.session_id,
    })
}
