use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};

use command::blocking::Command;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::schema::{
    KillTerminalResponse, ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalResponse,
    RequestPermissionRequest, RequestPermissionResponse, TerminalExitStatus,
    TerminalOutputResponse, TerminalRefRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
    WriteTextFileResponse,
};
use crate::{AgentMessage, JsonRpcErrorObject, JsonRpcTransportError, JsonRpcTransportHandle};

const ERROR_DISABLED: i64 = -32001;
const ERROR_PERMISSION_DENIED: i64 = -32002;
const ERROR_INVALID_PARAMS: i64 = -32602;
const ERROR_INTERNAL: i64 = -32603;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalClientRequestPolicy {
    pub workspace_root: PathBuf,
    pub allow_read_text_file: bool,
    pub allow_write_text_file: bool,
    pub allow_terminal: bool,
    pub allow_permission_selection: bool,
}

impl LocalClientRequestPolicy {
    #[must_use]
    pub fn conservative(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            allow_read_text_file: false,
            allow_write_text_file: false,
            allow_terminal: false,
            allow_permission_selection: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum LocalClientRequestError {
    #[error("invalid ACP client request parameters: {0}")]
    InvalidParams(#[from] serde_json::Error),
    #[error("ACP path `{path}` is outside workspace `{workspace}`")]
    OutsideWorkspace { path: PathBuf, workspace: PathBuf },
    #[error("ACP client capability `{0}` is disabled")]
    Disabled(&'static str),
    #[error("ACP client request `{0}` was denied by user")]
    PermissionDenied(&'static str),
    #[error("ACP terminal `{0}` was not found")]
    UnknownTerminal(String),
    #[error("I/O error while handling ACP client request: {0}")]
    Io(#[from] std::io::Error),
}

impl LocalClientRequestError {
    fn json_rpc_code(&self) -> i64 {
        match self {
            Self::InvalidParams(_) => ERROR_INVALID_PARAMS,
            Self::OutsideWorkspace { .. } => ERROR_PERMISSION_DENIED,
            Self::Disabled(_) => ERROR_DISABLED,
            Self::PermissionDenied(_) => ERROR_PERMISSION_DENIED,
            Self::UnknownTerminal(_) => ERROR_INVALID_PARAMS,
            Self::Io(_) => ERROR_INTERNAL,
        }
    }
}

struct LocalTerminal {
    child: Mutex<Child>,
    output: Arc<Mutex<Vec<u8>>>,
    output_limit: usize,
    exit_status: Mutex<Option<TerminalExitStatus>>,
}

pub struct LocalClientRequestHandler {
    policy: LocalClientRequestPolicy,
    workspace_root: PathBuf,
    terminals: HashMap<String, LocalTerminal>,
    next_terminal_id: u64,
    ui: Arc<dyn ClientRequestUi>,
}

pub trait ClientRequestUi: Send + Sync {
    fn approve_read_text_file(
        &self,
        _request: &ReadTextFileRequest,
        _resolved_path: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        Ok(false)
    }

    fn approve_write_text_file(
        &self,
        _request: &WriteTextFileRequest,
        _resolved_path: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        Ok(false)
    }

    fn request_permission(
        &self,
        request: &RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, LocalClientRequestError>;

    fn approve_terminal(
        &self,
        _request: &crate::schema::CreateTerminalRequest,
        _resolved_cwd: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        Ok(false)
    }
}

#[derive(Default)]
struct DenyClientRequestUi;

impl ClientRequestUi for DenyClientRequestUi {
    fn request_permission(
        &self,
        _request: &RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, LocalClientRequestError> {
        Ok(RequestPermissionResponse::cancelled())
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct AutoClientRequestUi;

#[cfg(test)]
impl ClientRequestUi for AutoClientRequestUi {
    fn approve_read_text_file(
        &self,
        _request: &ReadTextFileRequest,
        _resolved_path: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        Ok(true)
    }

    fn approve_write_text_file(
        &self,
        _request: &WriteTextFileRequest,
        _resolved_path: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        Ok(true)
    }

    fn request_permission(
        &self,
        request: &RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, LocalClientRequestError> {
        let selected = request
            .options
            .iter()
            .find(|option| {
                let label = option.name.to_lowercase();
                label.contains("allow") || label.contains("approve") || label.contains("yes")
            })
            .or_else(|| request.options.first());
        Ok(match selected {
            Some(option) => RequestPermissionResponse {
                outcome: crate::schema::RequestPermissionOutcome::Selected {
                    option_id: option.option_id.clone(),
                },
            },
            None => RequestPermissionResponse::cancelled(),
        })
    }

    fn approve_terminal(
        &self,
        _request: &crate::schema::CreateTerminalRequest,
        _resolved_cwd: &Path,
    ) -> Result<bool, LocalClientRequestError> {
        Ok(true)
    }
}

impl LocalClientRequestHandler {
    pub fn new(policy: LocalClientRequestPolicy) -> Result<Self, LocalClientRequestError> {
        let workspace_root = canonicalize_existing_dir(&policy.workspace_root)?;
        Ok(Self {
            policy,
            workspace_root,
            terminals: HashMap::new(),
            next_terminal_id: 1,
            ui: Arc::new(DenyClientRequestUi),
        })
    }

    #[must_use]
    pub fn with_ui(mut self, ui: Arc<dyn ClientRequestUi>) -> Self {
        self.ui = ui;
        self
    }

    pub fn handle(
        &mut self,
        message: AgentMessage,
        transport: &dyn JsonRpcTransportHandle,
    ) -> Result<(), JsonRpcTransportError> {
        let AgentMessage::Request { id, method, params } = message else {
            return Ok(());
        };

        let response = match method.as_str() {
            "fs/read_text_file" => self.read_text_file(params).map(to_json_value),
            "fs/write_text_file" => self.write_text_file(params).map(to_json_value),
            "session/request_permission" => self.request_permission(params).map(to_json_value),
            "terminal/create" => self.create_terminal(params).map(to_json_value),
            "terminal/output" => self.terminal_output(params).map(to_json_value),
            "terminal/wait_for_exit" => self.wait_for_terminal_exit(params).map(to_json_value),
            "terminal/kill" => self.kill_terminal(params).map(to_json_value),
            "terminal/release" => self.release_terminal(params).map(to_json_value),
            _ => {
                return transport.respond_error_object(
                    id,
                    JsonRpcErrorObject::new(
                        -32601,
                        format!("Unknown ACP client method `{method}`"),
                    ),
                );
            }
        };

        match response {
            Ok(value) => transport.respond_result_value(id, value),
            Err(error) => transport.respond_error_object(
                id,
                JsonRpcErrorObject::new(error.json_rpc_code(), error.to_string()),
            ),
        }
    }

    fn read_text_file(
        &self,
        params: Value,
    ) -> Result<ReadTextFileResponse, LocalClientRequestError> {
        if !self.policy.allow_read_text_file {
            return Err(LocalClientRequestError::Disabled("fs/read_text_file"));
        }
        let request: ReadTextFileRequest = serde_json::from_value(params)?;
        let path = self.resolve_workspace_path(&request.path, true)?;
        if !self.ui.approve_read_text_file(&request, &path)? {
            return Err(LocalClientRequestError::PermissionDenied(
                "fs/read_text_file",
            ));
        }
        let content = fs::read_to_string(path)?;
        Ok(ReadTextFileResponse {
            content: slice_lines(&content, request.line, request.limit),
        })
    }

    fn write_text_file(
        &self,
        params: Value,
    ) -> Result<WriteTextFileResponse, LocalClientRequestError> {
        if !self.policy.allow_write_text_file {
            return Err(LocalClientRequestError::Disabled("fs/write_text_file"));
        }
        let request: WriteTextFileRequest = serde_json::from_value(params)?;
        let path = self.resolve_workspace_path(&request.path, false)?;
        if !self.ui.approve_write_text_file(&request, &path)? {
            return Err(LocalClientRequestError::PermissionDenied(
                "fs/write_text_file",
            ));
        }
        fs::write(path, request.content)?;
        Ok(WriteTextFileResponse {})
    }

    fn request_permission(
        &self,
        params: Value,
    ) -> Result<RequestPermissionResponse, LocalClientRequestError> {
        let request: RequestPermissionRequest = serde_json::from_value(params)?;
        if !self.policy.allow_permission_selection {
            return Ok(RequestPermissionResponse::cancelled());
        }
        self.ui.request_permission(&request)
    }

    fn create_terminal(
        &mut self,
        params: Value,
    ) -> Result<crate::schema::CreateTerminalResponse, LocalClientRequestError> {
        if !self.policy.allow_terminal {
            return Err(LocalClientRequestError::Disabled("terminal/create"));
        }
        let request: crate::schema::CreateTerminalRequest = serde_json::from_value(params)?;
        let cwd = match &request.cwd {
            Some(cwd) => self.resolve_workspace_path(cwd, true)?,
            None => self.workspace_root.clone(),
        };
        if !self.ui.approve_terminal(&request, &cwd)? {
            return Err(LocalClientRequestError::PermissionDenied("terminal/create"));
        }
        let mut command = Command::new(&request.command);
        command
            .args(&request.args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for env in request.env {
            command.env(env.name, env.value);
        }
        let mut child = command.spawn()?;
        let output = Arc::new(Mutex::new(Vec::new()));
        if let Some(stdout) = child.stdout.take() {
            spawn_output_reader(stdout, Arc::clone(&output));
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_output_reader(stderr, Arc::clone(&output));
        }
        let terminal_id = format!("warp-acp-terminal-{}", self.next_terminal_id);
        self.next_terminal_id += 1;
        self.terminals.insert(
            terminal_id.clone(),
            LocalTerminal {
                child: Mutex::new(child),
                output,
                output_limit: request.output_byte_limit.unwrap_or(64 * 1024) as usize,
                exit_status: Mutex::new(None),
            },
        );
        Ok(crate::schema::CreateTerminalResponse { terminal_id })
    }

    fn terminal_output(
        &mut self,
        params: Value,
    ) -> Result<TerminalOutputResponse, LocalClientRequestError> {
        let request: TerminalRefRequest = serde_json::from_value(params)?;
        let terminal = self.terminal(&request.terminal_id)?;
        let exit_status = terminal_exit_status(terminal)?;
        let output = terminal
            .output
            .lock()
            .expect("ACP terminal output poisoned");
        let truncated = output.len() > terminal.output_limit;
        let start = output.len().saturating_sub(terminal.output_limit);
        let output = String::from_utf8_lossy(&output[start..]).to_string();
        Ok(TerminalOutputResponse {
            output,
            truncated,
            exit_status,
        })
    }

    fn wait_for_terminal_exit(
        &mut self,
        params: Value,
    ) -> Result<WaitForTerminalExitResponse, LocalClientRequestError> {
        let request: TerminalRefRequest = serde_json::from_value(params)?;
        let terminal = self.terminal(&request.terminal_id)?;
        if let Some(status) = terminal
            .exit_status
            .lock()
            .expect("ACP terminal status poisoned")
            .clone()
        {
            return Ok(WaitForTerminalExitResponse {
                exit_code: status.exit_code,
                signal: status.signal,
            });
        }
        let status = terminal
            .child
            .lock()
            .expect("ACP terminal child poisoned")
            .wait()?;
        let status = TerminalExitStatus {
            exit_code: status.code().map(|code| code as u32),
            signal: None,
        };
        *terminal
            .exit_status
            .lock()
            .expect("ACP terminal status poisoned") = Some(status.clone());
        Ok(WaitForTerminalExitResponse {
            exit_code: status.exit_code,
            signal: status.signal,
        })
    }

    fn kill_terminal(
        &mut self,
        params: Value,
    ) -> Result<KillTerminalResponse, LocalClientRequestError> {
        let request: TerminalRefRequest = serde_json::from_value(params)?;
        let terminal = self.terminal(&request.terminal_id)?;
        let mut child = terminal.child.lock().expect("ACP terminal child poisoned");
        let status = match child.try_wait()? {
            Some(status) => status,
            None => {
                child.kill()?;
                child.wait()?
            }
        };
        *terminal
            .exit_status
            .lock()
            .expect("ACP terminal status poisoned") = Some(TerminalExitStatus {
            exit_code: status.code().map(|code| code as u32),
            signal: None,
        });
        Ok(KillTerminalResponse {})
    }

    fn release_terminal(
        &mut self,
        params: Value,
    ) -> Result<ReleaseTerminalResponse, LocalClientRequestError> {
        let request: TerminalRefRequest = serde_json::from_value(params)?;
        if let Some(terminal) = self.terminals.remove(&request.terminal_id) {
            let mut child = terminal.child.lock().expect("ACP terminal child poisoned");
            if child.try_wait()?.is_none() {
                let _ = child.kill();
            }
            Ok(ReleaseTerminalResponse {})
        } else {
            Err(LocalClientRequestError::UnknownTerminal(
                request.terminal_id,
            ))
        }
    }

    fn terminal(&self, terminal_id: &str) -> Result<&LocalTerminal, LocalClientRequestError> {
        self.terminals
            .get(terminal_id)
            .ok_or_else(|| LocalClientRequestError::UnknownTerminal(terminal_id.to_string()))
    }

    fn resolve_workspace_path(
        &self,
        path: &Path,
        must_exist: bool,
    ) -> Result<PathBuf, LocalClientRequestError> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace_root.join(path)
        };
        let resolved = if must_exist {
            absolute.canonicalize()?
        } else {
            let parent = absolute
                .parent()
                .ok_or_else(|| LocalClientRequestError::OutsideWorkspace {
                    path: absolute.clone(),
                    workspace: self.workspace_root.clone(),
                })?
                .canonicalize()?;
            parent.join(absolute.file_name().ok_or_else(|| {
                LocalClientRequestError::OutsideWorkspace {
                    path: absolute.clone(),
                    workspace: self.workspace_root.clone(),
                }
            })?)
        };
        if resolved.starts_with(&self.workspace_root) {
            Ok(resolved)
        } else {
            Err(LocalClientRequestError::OutsideWorkspace {
                path: resolved,
                workspace: self.workspace_root.clone(),
            })
        }
    }
}

impl Drop for LocalClientRequestHandler {
    fn drop(&mut self) {
        for terminal in self.terminals.values() {
            if let Ok(mut child) = terminal.child.lock() {
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                }
            }
        }
    }
}

fn to_json_value<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn canonicalize_existing_dir(path: &Path) -> Result<PathBuf, LocalClientRequestError> {
    Ok(path.canonicalize()?)
}

fn slice_lines(content: &str, line: Option<u64>, limit: Option<u64>) -> String {
    if line.is_none() && limit.is_none() {
        return content.to_string();
    }
    let start = line.unwrap_or(1).saturating_sub(1) as usize;
    let iter = content.lines().skip(start);
    match limit {
        Some(limit) => iter.take(limit as usize).collect::<Vec<_>>().join("\n"),
        None => iter.collect::<Vec<_>>().join("\n"),
    }
}

fn spawn_output_reader(mut reader: impl Read + Send + 'static, output: Arc<Mutex<Vec<u8>>>) {
    std::thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => output
                    .lock()
                    .expect("ACP terminal output poisoned")
                    .extend_from_slice(&buf[..n]),
            }
        }
    });
}

fn terminal_exit_status(
    terminal: &LocalTerminal,
) -> Result<Option<TerminalExitStatus>, LocalClientRequestError> {
    if let Some(status) = terminal
        .exit_status
        .lock()
        .expect("ACP terminal status poisoned")
        .clone()
    {
        return Ok(Some(status));
    }

    let mut child = terminal.child.lock().expect("ACP terminal child poisoned");
    let Some(status) = child.try_wait()? else {
        return Ok(None);
    };
    let status = TerminalExitStatus {
        exit_code: status.code().map(|code| code as u32),
        signal: None,
    };
    *terminal
        .exit_status
        .lock()
        .expect("ACP terminal status poisoned") = Some(status.clone());
    Ok(Some(status))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use super::*;
    use crate::{JsonRpcId, JsonRpcStdioTransport};

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn temp_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "warp-acp-local-handler-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn reads_workspace_file_with_line_window() {
        let dir = temp_dir();
        fs::write(dir.join("note.txt"), "a\nb\nc\n").unwrap();
        let mut handler = LocalClientRequestHandler::new(LocalClientRequestPolicy {
            workspace_root: dir.clone(),
            allow_read_text_file: true,
            allow_write_text_file: false,
            allow_terminal: false,
            allow_permission_selection: false,
        })
        .unwrap()
        .with_ui(Arc::new(AutoClientRequestUi));
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let transport =
            JsonRpcStdioTransport::from_reader_writer(Cursor::new(Vec::new()), writer, None);

        handler
            .handle(
                AgentMessage::Request {
                    id: JsonRpcId::Number(7),
                    method: "fs/read_text_file".to_string(),
                    params: json!({"sessionId":"s","path":"note.txt","line":2,"limit":1}),
                },
                &transport,
            )
            .unwrap();

        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains(r#""id":7"#));
        assert!(written.contains(r#""content":"b""#));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn denies_path_outside_workspace() {
        let dir = temp_dir();
        let mut handler = LocalClientRequestHandler::new(LocalClientRequestPolicy {
            workspace_root: dir.clone(),
            allow_read_text_file: true,
            allow_write_text_file: false,
            allow_terminal: false,
            allow_permission_selection: false,
        })
        .unwrap()
        .with_ui(Arc::new(AutoClientRequestUi));
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let transport =
            JsonRpcStdioTransport::from_reader_writer(Cursor::new(Vec::new()), writer, None);

        handler
            .handle(
                AgentMessage::Request {
                    id: JsonRpcId::Number(8),
                    method: "fs/read_text_file".to_string(),
                    params: json!({"sessionId":"s","path":"/etc/passwd"}),
                },
                &transport,
            )
            .unwrap();

        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains(r#""code":-32002"#));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn writes_workspace_file_when_enabled() {
        let dir = temp_dir();
        let mut handler = LocalClientRequestHandler::new(LocalClientRequestPolicy {
            workspace_root: dir.clone(),
            allow_read_text_file: false,
            allow_write_text_file: true,
            allow_terminal: false,
            allow_permission_selection: false,
        })
        .unwrap()
        .with_ui(Arc::new(AutoClientRequestUi));
        let transport = JsonRpcStdioTransport::from_reader_writer(
            Cursor::new(Vec::new()),
            SharedWriter::default(),
            None,
        );

        handler
            .handle(
                AgentMessage::Request {
                    id: JsonRpcId::Number(9),
                    method: "fs/write_text_file".to_string(),
                    params: json!({"sessionId":"s","path":"created.txt","content":"hello"}),
                },
                &transport,
            )
            .unwrap();

        assert_eq!(
            fs::read_to_string(dir.join("created.txt")).unwrap(),
            "hello"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[derive(Default)]
    struct RecordingUi {
        reads: Arc<Mutex<Vec<PathBuf>>>,
        writes: Arc<Mutex<Vec<PathBuf>>>,
        allow_read: bool,
        allow_write: bool,
    }

    impl ClientRequestUi for RecordingUi {
        fn approve_read_text_file(
            &self,
            _request: &ReadTextFileRequest,
            resolved_path: &Path,
        ) -> Result<bool, LocalClientRequestError> {
            self.reads.lock().unwrap().push(resolved_path.to_path_buf());
            Ok(self.allow_read)
        }

        fn approve_write_text_file(
            &self,
            _request: &WriteTextFileRequest,
            resolved_path: &Path,
        ) -> Result<bool, LocalClientRequestError> {
            self.writes
                .lock()
                .unwrap()
                .push(resolved_path.to_path_buf());
            Ok(self.allow_write)
        }

        fn request_permission(
            &self,
            _request: &RequestPermissionRequest,
        ) -> Result<RequestPermissionResponse, LocalClientRequestError> {
            Ok(RequestPermissionResponse::cancelled())
        }
    }

    #[test]
    fn read_request_waits_for_ui_decision() {
        let dir = temp_dir();
        let file = dir.join("readable.txt");
        fs::write(&file, "secret").unwrap();
        let ui = Arc::new(RecordingUi {
            reads: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::new(Mutex::new(Vec::new())),
            allow_read: false,
            allow_write: true,
        });
        let reads = Arc::clone(&ui.reads);
        let mut handler = LocalClientRequestHandler::new(LocalClientRequestPolicy {
            workspace_root: dir.clone(),
            allow_read_text_file: true,
            allow_write_text_file: false,
            allow_terminal: false,
            allow_permission_selection: false,
        })
        .unwrap()
        .with_ui(ui);
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let transport =
            JsonRpcStdioTransport::from_reader_writer(Cursor::new(Vec::new()), writer, None);

        handler
            .handle(
                AgentMessage::Request {
                    id: JsonRpcId::Number(11),
                    method: "fs/read_text_file".to_string(),
                    params: json!({"sessionId":"s","path":"readable.txt"}),
                },
                &transport,
            )
            .unwrap();

        assert_eq!(
            reads.lock().unwrap().as_slice(),
            &[file.canonicalize().unwrap()]
        );
        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains(r#""id":11"#));
        assert!(written.contains(r#""code":-32002"#));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn default_ui_denies_enabled_capabilities_without_explicit_ui() {
        let dir = temp_dir();
        fs::write(dir.join("readable.txt"), "secret").unwrap();
        let mut handler = LocalClientRequestHandler::new(LocalClientRequestPolicy {
            workspace_root: dir.clone(),
            allow_read_text_file: true,
            allow_write_text_file: false,
            allow_terminal: false,
            allow_permission_selection: false,
        })
        .unwrap();
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let transport =
            JsonRpcStdioTransport::from_reader_writer(Cursor::new(Vec::new()), writer, None);

        handler
            .handle(
                AgentMessage::Request {
                    id: JsonRpcId::Number(12),
                    method: "fs/read_text_file".to_string(),
                    params: json!({"sessionId":"s","path":"readable.txt"}),
                },
                &transport,
            )
            .unwrap();

        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains(r#""code":-32002"#));
        assert!(!written.contains("secret"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn write_request_waits_for_ui_decision() {
        let dir = temp_dir();
        let ui = Arc::new(RecordingUi {
            reads: Arc::new(Mutex::new(Vec::new())),
            writes: Arc::new(Mutex::new(Vec::new())),
            allow_read: true,
            allow_write: false,
        });
        let writes = Arc::clone(&ui.writes);
        let mut handler = LocalClientRequestHandler::new(LocalClientRequestPolicy {
            workspace_root: dir.clone(),
            allow_read_text_file: false,
            allow_write_text_file: true,
            allow_terminal: false,
            allow_permission_selection: false,
        })
        .unwrap()
        .with_ui(ui);
        let writer = SharedWriter::default();
        let captured = Arc::clone(&writer.0);
        let transport =
            JsonRpcStdioTransport::from_reader_writer(Cursor::new(Vec::new()), writer, None);

        handler
            .handle(
                AgentMessage::Request {
                    id: JsonRpcId::Number(10),
                    method: "fs/write_text_file".to_string(),
                    params: json!({"sessionId":"s","path":"created.txt","content":"hello"}),
                },
                &transport,
            )
            .unwrap();

        assert_eq!(writes.lock().unwrap().len(), 1);
        assert!(!dir.join("created.txt").exists());
        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains(r#""id":10"#));
        assert!(written.contains(r#""code":-32002"#));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn runs_terminal_and_reports_output_and_exit() {
        let dir = temp_dir();
        let mut handler = LocalClientRequestHandler::new(LocalClientRequestPolicy {
            workspace_root: dir.clone(),
            allow_read_text_file: false,
            allow_write_text_file: false,
            allow_terminal: true,
            allow_permission_selection: false,
        })
        .unwrap()
        .with_ui(Arc::new(AutoClientRequestUi));

        let terminal_id = handler
            .create_terminal(json!({
                "sessionId":"s",
                "command":"/bin/echo",
                "args":["hello-acp"],
                "cwd": dir,
                "env": []
            }))
            .unwrap()
            .terminal_id;
        let exit = handler
            .wait_for_terminal_exit(json!({"sessionId":"s","terminalId":terminal_id}))
            .unwrap();
        assert_eq!(exit.exit_code, Some(0));
        std::thread::sleep(std::time::Duration::from_millis(20));
        let output = handler
            .terminal_output(json!({"sessionId":"s","terminalId":terminal_id}))
            .unwrap();
        assert!(output.output.contains("hello-acp"));
        assert_eq!(output.exit_status.unwrap().exit_code, Some(0));
        let _ = handler.release_terminal(json!({"sessionId":"s","terminalId":terminal_id}));
        let _ = fs::remove_dir_all(dir);
    }
}
