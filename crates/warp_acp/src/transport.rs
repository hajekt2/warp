use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::command::{AcpAgentCommand, AcpCommandError};
use crate::jsonrpc::{
    decode_frame, encode_frame, AgentMessage, IncomingFrame, JsonRpcErrorObject,
    JsonRpcErrorResponse, JsonRpcId, JsonRpcNotification, JsonRpcRequest, JsonRpcResult,
};

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

type PendingSender = mpsc::Sender<Result<Value, JsonRpcErrorObject>>;

#[derive(Debug, Error)]
pub enum JsonRpcTransportError {
    #[error("failed to validate ACP command: {0}")]
    Command(#[from] AcpCommandError),
    #[error("failed to spawn ACP agent: {0}")]
    Spawn(std::io::Error),
    #[error("ACP agent subprocess did not expose piped stdio")]
    MissingPipe,
    #[error("failed to encode JSON-RPC frame: {0}")]
    Encode(serde_json::Error),
    #[error("failed to decode JSON-RPC frame: {0}")]
    Decode(serde_json::Error),
    #[error("failed to write JSON-RPC frame: {0}")]
    Write(std::io::Error),
    #[error("ACP request `{method}` timed out after {timeout:?}")]
    Timeout { method: String, timeout: Duration },
    #[error("ACP transport closed before response")]
    Closed,
    #[error("ACP agent returned error {code}: {message}")]
    RemoteError {
        code: i64,
        message: String,
        data: Option<Value>,
    },
}

pub struct JsonRpcStdioTransport {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pending: Arc<Mutex<HashMap<u64, PendingSender>>>,
    inbound_rx: Mutex<mpsc::Receiver<AgentMessage>>,
    next_request_id: AtomicU64,
    closed: Arc<AtomicBool>,
    child: Mutex<Option<Child>>,
}

impl JsonRpcStdioTransport {
    pub fn spawn(config: &AcpAgentCommand) -> Result<Self, JsonRpcTransportError> {
        let mut command = config.to_std_command()?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(JsonRpcTransportError::Spawn)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(JsonRpcTransportError::MissingPipe)?;
        let stdin = child
            .stdin
            .take()
            .ok_or(JsonRpcTransportError::MissingPipe)?;
        Ok(Self::from_reader_writer(stdout, stdin, Some(child)))
    }

    pub fn from_reader_writer<R, W>(reader: R, writer: W, child: Option<Child>) -> Self
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(writer)));
        let pending: Arc<Mutex<HashMap<u64, PendingSender>>> = Arc::new(Mutex::new(HashMap::new()));
        let (inbound_tx, inbound_rx) = mpsc::channel();
        let pending_for_thread = Arc::clone(&pending);
        let closed = Arc::new(AtomicBool::new(false));
        let closed_for_thread = Arc::clone(&closed);

        thread::spawn(move || {
            let reader = BufReader::new(reader);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                match decode_frame(&line) {
                    Ok(IncomingFrame::Response { id, result }) => {
                        if let Some(tx) = pending_for_thread
                            .lock()
                            .ok()
                            .and_then(|mut p| p.remove(&id))
                        {
                            let _ = tx.send(result);
                        }
                    }
                    Ok(IncomingFrame::AgentMessage(message)) => {
                        let _ = inbound_tx.send(message);
                    }
                    Err(error) => {
                        let _ = inbound_tx.send(AgentMessage::Notification {
                            method: "$/warp/malformedFrame".to_string(),
                            params: Value::String(error.to_string()),
                        });
                    }
                }
            }
            // EOF means the subprocess exited or closed stdout. Drop all pending
            // response senders so blocked requests return `Closed` immediately
            // instead of waiting for their full timeout.
            closed_for_thread.store(true, Ordering::Relaxed);
            if let Ok(mut pending) = pending_for_thread.lock() {
                pending.clear();
            }
        });

        Self {
            writer,
            pending,
            inbound_rx: Mutex::new(inbound_rx),
            next_request_id: AtomicU64::new(1),
            closed,
            child: Mutex::new(child),
        }
    }

    pub fn request<P, R>(&self, method: &str, params: P) -> Result<R, JsonRpcTransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        self.request_timeout(method, params, DEFAULT_REQUEST_TIMEOUT)
    }

    pub fn request_timeout<P, R>(
        &self,
        method: &str,
        params: P,
        timeout: Duration,
    ) -> Result<R, JsonRpcTransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
    {
        self.request_timeout_with_handler(method, params, timeout, |_, _| Ok(()))
    }

    pub fn request_timeout_with_handler<P, R, F>(
        &self,
        method: &str,
        params: P,
        timeout: Duration,
        mut handle_agent_message: F,
    ) -> Result<R, JsonRpcTransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
        F: FnMut(AgentMessage, &Self) -> Result<(), JsonRpcTransportError>,
    {
        if self.closed.load(Ordering::Relaxed) {
            return Err(JsonRpcTransportError::Closed);
        }

        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending
            .lock()
            .expect("pending poisoned")
            .insert(id, tx);

        let frame = JsonRpcRequest::new(JsonRpcId::Number(id), method, params);
        if let Err(error) = self.write_frame(&frame) {
            self.pending.lock().expect("pending poisoned").remove(&id);
            return Err(error);
        }

        let started_at = Instant::now();
        loop {
            self.drain_agent_messages(&mut handle_agent_message)?;
            if self.closed.load(Ordering::Relaxed) {
                self.pending.lock().expect("pending poisoned").remove(&id);
                return Err(JsonRpcTransportError::Closed);
            }
            let elapsed = started_at.elapsed();
            if elapsed >= timeout {
                self.pending.lock().expect("pending poisoned").remove(&id);
                return Err(JsonRpcTransportError::Timeout {
                    method: method.to_string(),
                    timeout,
                });
            }
            let wait = (timeout - elapsed).min(Duration::from_millis(50));
            match rx.recv_timeout(wait) {
                Ok(Ok(value)) => {
                    self.drain_agent_messages(&mut handle_agent_message)?;
                    return serde_json::from_value(value).map_err(JsonRpcTransportError::Decode);
                }
                Ok(Err(error)) => {
                    self.drain_agent_messages(&mut handle_agent_message)?;
                    return Err(JsonRpcTransportError::RemoteError {
                        code: error.code,
                        message: error.message,
                        data: error.data,
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(JsonRpcTransportError::Closed)
                }
            }
        }
    }

    pub fn notify<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> Result<(), JsonRpcTransportError> {
        self.write_frame(&JsonRpcNotification::new(method, params))
    }

    pub fn respond_result<R: Serialize>(
        &self,
        id: JsonRpcId,
        result: R,
    ) -> Result<(), JsonRpcTransportError> {
        self.write_frame(&JsonRpcResult::new(id, result))
    }

    pub fn respond_error(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), JsonRpcTransportError> {
        self.write_frame(&JsonRpcErrorResponse::new(id, error))
    }

    pub fn recv_message(
        &self,
        timeout: Duration,
    ) -> Result<Option<AgentMessage>, JsonRpcTransportError> {
        match self
            .inbound_rx
            .lock()
            .expect("inbound poisoned")
            .recv_timeout(timeout)
        {
            Ok(message) => Ok(Some(message)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(JsonRpcTransportError::Closed),
        }
    }

    pub fn kill_child(&self) -> Result<(), std::io::Error> {
        if let Some(child) = self.child.lock().expect("child poisoned").as_mut() {
            child.kill()?;
        }
        Ok(())
    }

    fn drain_agent_messages<F>(&self, handler: &mut F) -> Result<(), JsonRpcTransportError>
    where
        F: FnMut(AgentMessage, &Self) -> Result<(), JsonRpcTransportError>,
    {
        loop {
            let message = {
                let rx = self.inbound_rx.lock().expect("inbound poisoned");
                rx.try_recv()
            };
            match message {
                Ok(message) => handler(message, self)?,
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }

    fn write_frame<T: Serialize>(&self, frame: &T) -> Result<(), JsonRpcTransportError> {
        let bytes = encode_frame(frame).map_err(JsonRpcTransportError::Encode)?;
        let mut writer = self.writer.lock().expect("writer poisoned");
        writer
            .write_all(&bytes)
            .map_err(JsonRpcTransportError::Write)?;
        writer.flush().map_err(JsonRpcTransportError::Write)
    }
}

impl Drop for JsonRpcStdioTransport {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        if let Ok(mut child) = self.child.lock() {
            if let Some(child) = child.as_mut() {
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};
    use std::sync::{Arc, Mutex};

    use serde_json::json;

    use super::*;

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

    #[test]
    fn request_correlates_response_by_id() {
        let input = Cursor::new(
            br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}
"#
            .to_vec(),
        );
        let output = SharedWriter::default();
        let captured = Arc::clone(&output.0);
        let transport = JsonRpcStdioTransport::from_reader_writer(input, output, None);

        let result: serde_json::Value = transport
            .request_timeout("initialize", json!({}), Duration::from_secs(1))
            .unwrap();

        assert_eq!(result, json!({"ok": true}));
        let written = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
        assert!(written.contains("initialize"));
        assert!(written.ends_with('\n'));
    }

    #[test]
    fn receives_agent_notifications() {
        let input = Cursor::new(
            br#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s"}}
"#
            .to_vec(),
        );
        let transport =
            JsonRpcStdioTransport::from_reader_writer(input, SharedWriter::default(), None);

        let message = transport
            .recv_message(Duration::from_secs(1))
            .unwrap()
            .unwrap();

        assert_eq!(
            message,
            AgentMessage::Notification {
                method: "session/update".to_string(),
                params: json!({"sessionId": "s"}),
            }
        );
    }

    struct SlowEofReader;

    impl Read for SlowEofReader {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(50));
            Ok(0)
        }
    }

    #[test]
    fn request_times_out_and_clears_pending_entry() {
        let transport =
            JsonRpcStdioTransport::from_reader_writer(SlowEofReader, SharedWriter::default(), None);

        let error = transport
            .request_timeout::<_, Value>("initialize", json!({}), Duration::from_millis(5))
            .unwrap_err();

        assert!(matches!(error, JsonRpcTransportError::Timeout { .. }));
        assert!(transport.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn request_returns_closed_when_reader_ends_before_response() {
        let transport = JsonRpcStdioTransport::from_reader_writer(
            Cursor::new(Vec::new()),
            SharedWriter::default(),
            None,
        );

        let error = transport
            .request_timeout::<_, Value>("initialize", json!({}), Duration::from_secs(1))
            .unwrap_err();

        assert!(matches!(error, JsonRpcTransportError::Closed));
        assert!(transport.pending.lock().unwrap().is_empty());
    }

    #[test]
    fn malformed_json_surfaces_diagnostic_notification() {
        let input = Cursor::new(b"{not json}\n".to_vec());
        let transport =
            JsonRpcStdioTransport::from_reader_writer(input, SharedWriter::default(), None);

        let message = transport
            .recv_message(Duration::from_secs(1))
            .unwrap()
            .unwrap();

        match message {
            AgentMessage::Notification { method, params } => {
                assert_eq!(method, "$/warp/malformedFrame");
                assert!(params.as_str().unwrap_or_default().contains("key"));
            }
            other => panic!("expected malformed-frame notification, got {other:?}"),
        }
    }
}
