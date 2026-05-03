use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use futures::{SinkExt, StreamExt};

use instant::Instant;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::command::{AcpAgentCommand, AcpCommandError, AcpRemoteEndpoint};
use crate::jsonrpc::{
    decode_frame, encode_frame, AgentMessage, IncomingFrame, JsonRpcErrorObject,
    JsonRpcErrorResponse, JsonRpcId, JsonRpcNotification, JsonRpcRequest, JsonRpcResult,
};

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

type PendingSender = mpsc::Sender<Result<Value, JsonRpcErrorObject>>;

pub trait JsonRpcTransportHandle: Send + Sync {
    fn respond_result_value(
        &self,
        id: JsonRpcId,
        result: Value,
    ) -> Result<(), JsonRpcTransportError>;

    fn respond_error_object(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), JsonRpcTransportError>;
}

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
    #[error("invalid ACP remote header `{0}`")]
    InvalidHeader(String),
    #[error("ACP HTTP transport failed: {0}")]
    Http(String),
    #[error("ACP WebSocket transport is not available in this build: {0}")]
    WebSocket(String),
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
        if let Some(stderr) = child.stderr.take() {
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    if !line.trim().is_empty() {
                        log::warn!("ACP agent stderr: {line}");
                    }
                }
            });
        }
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
        handle_agent_message: F,
    ) -> Result<R, JsonRpcTransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
        F: FnMut(AgentMessage, &dyn JsonRpcTransportHandle) -> Result<(), JsonRpcTransportError>,
    {
        let params = serde_json::to_value(params).map_err(JsonRpcTransportError::Encode)?;
        let value =
            self.request_value_timeout_with_handler(method, params, timeout, handle_agent_message)?;
        serde_json::from_value(value).map_err(JsonRpcTransportError::Decode)
    }

    pub fn request_value_timeout_with_handler<F>(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        mut handle_agent_message: F,
    ) -> Result<Value, JsonRpcTransportError>
    where
        F: FnMut(AgentMessage, &dyn JsonRpcTransportHandle) -> Result<(), JsonRpcTransportError>,
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
            match rx.try_recv() {
                Ok(Ok(value)) => {
                    self.drain_agent_messages(&mut handle_agent_message)?;
                    return Ok(value);
                }
                Ok(Err(error)) => {
                    self.drain_agent_messages(&mut handle_agent_message)?;
                    return Err(JsonRpcTransportError::RemoteError {
                        code: error.code,
                        message: error.message,
                        data: error.data,
                    });
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return Err(JsonRpcTransportError::Closed),
            }
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
                    return Ok(value);
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
                    return Err(JsonRpcTransportError::Closed);
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
        let result = serde_json::to_value(result).map_err(JsonRpcTransportError::Encode)?;
        self.respond_result_value(id, result)
    }

    pub fn respond_error(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), JsonRpcTransportError> {
        self.respond_error_object(id, error)
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
            let _ = child.wait();
        }
        Ok(())
    }

    fn drain_agent_messages<F>(&self, handler: &mut F) -> Result<(), JsonRpcTransportError>
    where
        F: FnMut(AgentMessage, &dyn JsonRpcTransportHandle) -> Result<(), JsonRpcTransportError>,
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

/// Request/response JSON-RPC transport over HTTP POST. This is useful for
/// remote ACP endpoints that expose non-streaming JSON-RPC methods. For
/// streaming prompt output or agent-initiated client requests, prefer the
/// WebSocket transport.
pub struct JsonRpcHttpTransport {
    url: String,
    headers: reqwest::header::HeaderMap,
    client: reqwest::blocking::Client,
    next_request_id: AtomicU64,
}

impl JsonRpcHttpTransport {
    pub fn connect(endpoint: &AcpRemoteEndpoint) -> Result<Self, JsonRpcTransportError> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut headers = reqwest::header::HeaderMap::new();
        for header in &endpoint.headers {
            let name = reqwest::header::HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|_| JsonRpcTransportError::InvalidHeader(header.name.clone()))?;
            let value = reqwest::header::HeaderValue::from_str(&header.value)
                .map_err(|_| JsonRpcTransportError::InvalidHeader(header.name.clone()))?;
            headers.insert(name, value);
        }
        Ok(Self {
            url: endpoint.url.clone(),
            headers,
            client: reqwest::blocking::Client::new(),
            next_request_id: AtomicU64::new(1),
        })
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
        let params = serde_json::to_value(params).map_err(JsonRpcTransportError::Encode)?;
        let value = self.request_value(method, params, Some(timeout))?;
        serde_json::from_value(value).map_err(JsonRpcTransportError::Decode)
    }

    pub fn request_timeout_with_handler<P, R, F>(
        &self,
        method: &str,
        params: P,
        timeout: Duration,
        _handle_agent_message: F,
    ) -> Result<R, JsonRpcTransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
        F: FnMut(AgentMessage, &dyn JsonRpcTransportHandle) -> Result<(), JsonRpcTransportError>,
    {
        self.request_timeout(method, params, timeout)
    }

    pub fn notify<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> Result<(), JsonRpcTransportError> {
        let params = serde_json::to_value(params).map_err(JsonRpcTransportError::Encode)?;
        let frame = JsonRpcNotification::new(method, params);
        self.post_json(
            serde_json::to_value(frame).map_err(JsonRpcTransportError::Encode)?,
            None,
        )?;
        Ok(())
    }

    pub fn recv_message(
        &self,
        _timeout: Duration,
    ) -> Result<Option<AgentMessage>, JsonRpcTransportError> {
        Ok(None)
    }

    fn request_value(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, JsonRpcTransportError> {
        let id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let frame = JsonRpcRequest::new(JsonRpcId::Number(id), method, params);
        let response = self.post_json(
            serde_json::to_value(frame).map_err(JsonRpcTransportError::Encode)?,
            timeout,
        )?;
        match decode_frame(&response.to_string()).map_err(JsonRpcTransportError::Decode)? {
            IncomingFrame::Response {
                id: response_id,
                result,
            } if response_id == id => result.map_err(|error| JsonRpcTransportError::RemoteError {
                code: error.code,
                message: error.message,
                data: error.data,
            }),
            IncomingFrame::Response { .. } => Err(JsonRpcTransportError::Closed),
            IncomingFrame::AgentMessage(_) => Err(JsonRpcTransportError::Http(
                "HTTP ACP endpoint returned an agent message instead of a response".to_string(),
            )),
        }
    }

    fn post_json(
        &self,
        body: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, JsonRpcTransportError> {
        let mut request = self
            .client
            .post(&self.url)
            .headers(self.headers.clone())
            .json(&body);
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        let response = request
            .send()
            .map_err(|error| JsonRpcTransportError::Http(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(JsonRpcTransportError::Http(format!(
                "remote endpoint returned HTTP {status}"
            )));
        }
        response
            .json()
            .map_err(|error| JsonRpcTransportError::Http(error.to_string()))
    }
}

impl JsonRpcTransportHandle for JsonRpcHttpTransport {
    fn respond_result_value(
        &self,
        id: JsonRpcId,
        result: Value,
    ) -> Result<(), JsonRpcTransportError> {
        let frame = JsonRpcResult::new(id, result);
        self.post_json(
            serde_json::to_value(frame).map_err(JsonRpcTransportError::Encode)?,
            None,
        )?;
        Ok(())
    }

    fn respond_error_object(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), JsonRpcTransportError> {
        let frame = JsonRpcErrorResponse::new(id, error);
        self.post_json(
            serde_json::to_value(frame).map_err(JsonRpcTransportError::Encode)?,
            None,
        )?;
        Ok(())
    }
}

pub struct JsonRpcWebSocketTransport {
    writer_tx: mpsc::Sender<String>,
    pending: Arc<Mutex<HashMap<u64, PendingSender>>>,
    inbound_rx: Mutex<mpsc::Receiver<AgentMessage>>,
    next_request_id: AtomicU64,
    closed: Arc<AtomicBool>,
}

impl JsonRpcWebSocketTransport {
    pub fn connect(endpoint: &AcpRemoteEndpoint) -> Result<Self, JsonRpcTransportError> {
        let (writer_tx, writer_rx) = mpsc::channel::<String>();
        let pending: Arc<Mutex<HashMap<u64, PendingSender>>> = Arc::new(Mutex::new(HashMap::new()));
        let (inbound_tx, inbound_rx) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let pending_for_thread = Arc::clone(&pending);
        let closed_for_thread = Arc::clone(&closed);
        let endpoint = endpoint.clone();
        let (ready_tx, ready_rx) = mpsc::channel();

        thread::spawn(move || {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };

            runtime.block_on(async move {
                let result = async {
                    use websocket::tungstenite::client::IntoClientRequest as _;
                    use websocket::tungstenite::http::{HeaderName, HeaderValue};
                    use websocket::WebsocketMessage as _;

                    let mut request = endpoint
                        .url
                        .as_str()
                        .into_client_request()
                        .map_err(|error| error.to_string())?;
                    for header in &endpoint.headers {
                        let name = HeaderName::from_bytes(header.name.as_bytes())
                            .map_err(|_| format!("invalid header `{}`", header.name))?;
                        let value = HeaderValue::from_str(&header.value)
                            .map_err(|_| format!("invalid header `{}`", header.name))?;
                        request.headers_mut().insert(name, value);
                    }

                    let socket = websocket::WebSocket::connect(request, std::iter::empty())
                        .await
                        .map_err(|error| error.to_string())?;
                    let (mut sink, mut stream) = socket.split().await;
                    let (async_writer_tx, mut async_writer_rx) =
                        tokio::sync::mpsc::unbounded_channel::<String>();
                    let bridge_closed = Arc::clone(&closed_for_thread);
                    thread::spawn(move || {
                        while let Ok(frame) = writer_rx.recv() {
                            if bridge_closed.load(Ordering::Relaxed) {
                                break;
                            }
                            if async_writer_tx.send(frame).is_err() {
                                break;
                            }
                        }
                    });

                    let write_task = tokio::spawn(async move {
                        while let Some(frame) = async_writer_rx.recv().await {
                            if sink
                                .send(websocket::Message::new_text(frame))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    });
                    let _ = ready_tx.send(Ok(()));

                    while let Some(message) = stream.next().await {
                        let message = match message {
                            Ok(message) => message,
                            Err(error) => {
                                let _ = inbound_tx.send(AgentMessage::Notification {
                                    method: "$/warp/websocketError".to_string(),
                                    params: Value::String(error.to_string()),
                                });
                                break;
                            }
                        };
                        let Some(text) = message.text() else {
                            continue;
                        };
                        match decode_frame(text) {
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
                    write_task.abort();
                    Ok::<(), String>(())
                }
                .await;

                if let Err(error) = result {
                    let _ = ready_tx.send(Err(error));
                }
                closed_for_thread.store(true, Ordering::Relaxed);
                if let Ok(mut pending) = pending_for_thread.lock() {
                    pending.clear();
                }
            });
        });

        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                writer_tx,
                pending,
                inbound_rx: Mutex::new(inbound_rx),
                next_request_id: AtomicU64::new(1),
                closed,
            }),
            Ok(Err(error)) => Err(JsonRpcTransportError::WebSocket(error)),
            Err(_) => Err(JsonRpcTransportError::Timeout {
                method: "websocket/connect".to_string(),
                timeout: Duration::from_secs(10),
            }),
        }
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
        F: FnMut(AgentMessage, &dyn JsonRpcTransportHandle) -> Result<(), JsonRpcTransportError>,
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
        let params = serde_json::to_value(params).map_err(JsonRpcTransportError::Encode)?;
        let frame = JsonRpcRequest::new(JsonRpcId::Number(id), method, params);
        if let Err(error) = self.write_frame(&frame) {
            self.pending.lock().expect("pending poisoned").remove(&id);
            return Err(error);
        }

        let started_at = Instant::now();
        loop {
            self.drain_agent_messages(&mut handle_agent_message)?;
            match rx.try_recv() {
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
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return Err(JsonRpcTransportError::Closed),
            }
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
                    return Err(JsonRpcTransportError::Closed);
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

    fn drain_agent_messages<F>(&self, handler: &mut F) -> Result<(), JsonRpcTransportError>
    where
        F: FnMut(AgentMessage, &dyn JsonRpcTransportHandle) -> Result<(), JsonRpcTransportError>,
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
        let frame = String::from_utf8(bytes)
            .map_err(|error| JsonRpcTransportError::WebSocket(error.to_string()))?;
        self.writer_tx
            .send(frame.trim_end_matches('\n').to_string())
            .map_err(|_| JsonRpcTransportError::Closed)
    }
}

impl JsonRpcTransportHandle for JsonRpcWebSocketTransport {
    fn respond_result_value(
        &self,
        id: JsonRpcId,
        result: Value,
    ) -> Result<(), JsonRpcTransportError> {
        self.write_frame(&JsonRpcResult::new(id, result))
    }

    fn respond_error_object(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), JsonRpcTransportError> {
        self.write_frame(&JsonRpcErrorResponse::new(id, error))
    }
}

impl Drop for JsonRpcWebSocketTransport {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
    }
}

pub enum JsonRpcTransport {
    Stdio(JsonRpcStdioTransport),
    Http(JsonRpcHttpTransport),
    WebSocket(JsonRpcWebSocketTransport),
}

impl From<JsonRpcStdioTransport> for JsonRpcTransport {
    fn from(value: JsonRpcStdioTransport) -> Self {
        Self::Stdio(value)
    }
}

impl JsonRpcTransport {
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
        match self {
            Self::Stdio(transport) => transport.request_timeout(method, params, timeout),
            Self::Http(transport) => transport.request_timeout(method, params, timeout),
            Self::WebSocket(transport) => transport.request_timeout(method, params, timeout),
        }
    }

    pub fn request_timeout_with_handler<P, R, F>(
        &self,
        method: &str,
        params: P,
        timeout: Duration,
        handle_agent_message: F,
    ) -> Result<R, JsonRpcTransportError>
    where
        P: Serialize,
        R: DeserializeOwned,
        F: FnMut(AgentMessage, &dyn JsonRpcTransportHandle) -> Result<(), JsonRpcTransportError>,
    {
        match self {
            Self::Stdio(transport) => transport.request_timeout_with_handler(
                method,
                params,
                timeout,
                handle_agent_message,
            ),
            Self::Http(transport) => transport.request_timeout_with_handler(
                method,
                params,
                timeout,
                handle_agent_message,
            ),
            Self::WebSocket(transport) => transport.request_timeout_with_handler(
                method,
                params,
                timeout,
                handle_agent_message,
            ),
        }
    }

    pub fn notify<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> Result<(), JsonRpcTransportError> {
        match self {
            Self::Stdio(transport) => transport.notify(method, params),
            Self::Http(transport) => transport.notify(method, params),
            Self::WebSocket(transport) => transport.notify(method, params),
        }
    }

    pub fn recv_message(
        &self,
        timeout: Duration,
    ) -> Result<Option<AgentMessage>, JsonRpcTransportError> {
        match self {
            Self::Stdio(transport) => transport.recv_message(timeout),
            Self::Http(transport) => transport.recv_message(timeout),
            Self::WebSocket(transport) => transport.recv_message(timeout),
        }
    }
}

impl JsonRpcTransportHandle for JsonRpcTransport {
    fn respond_result_value(
        &self,
        id: JsonRpcId,
        result: Value,
    ) -> Result<(), JsonRpcTransportError> {
        match self {
            Self::Stdio(transport) => transport.respond_result_value(id, result),
            Self::Http(transport) => transport.respond_result_value(id, result),
            Self::WebSocket(transport) => transport.respond_result_value(id, result),
        }
    }

    fn respond_error_object(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), JsonRpcTransportError> {
        match self {
            Self::Stdio(transport) => transport.respond_error_object(id, error),
            Self::Http(transport) => transport.respond_error_object(id, error),
            Self::WebSocket(transport) => transport.respond_error_object(id, error),
        }
    }
}

impl JsonRpcTransportHandle for JsonRpcStdioTransport {
    fn respond_result_value(
        &self,
        id: JsonRpcId,
        result: Value,
    ) -> Result<(), JsonRpcTransportError> {
        self.write_frame(&JsonRpcResult::new(id, result))
    }

    fn respond_error_object(
        &self,
        id: JsonRpcId,
        error: JsonRpcErrorObject,
    ) -> Result<(), JsonRpcTransportError> {
        self.write_frame(&JsonRpcErrorResponse::new(id, error))
    }
}

impl Drop for JsonRpcStdioTransport {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
        if let Ok(mut child) = self.child.lock() {
            if let Some(child) = child.as_mut() {
                if matches!(child.try_wait(), Ok(None)) {
                    let _ = child.kill();
                    let _ = child.wait();
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
    fn http_transport_posts_json_rpc_requests() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.contains("initialize"));
            let body = br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(body).unwrap();
        });

        let endpoint = AcpRemoteEndpoint::new(format!("http://{addr}/acp"));
        let transport = JsonRpcHttpTransport::connect(&endpoint).unwrap();
        let result: Value = transport
            .request_timeout("initialize", json!({}), Duration::from_secs(1))
            .unwrap();
        assert_eq!(result, json!({"ok": true}));
        server.join().unwrap();
    }

    #[test]
    fn http_transport_recv_message_returns_immediately() {
        let endpoint = AcpRemoteEndpoint::new("http://127.0.0.1:9/acp");
        let transport = JsonRpcHttpTransport::connect(&endpoint).unwrap();
        let started_at = Instant::now();

        let message = transport.recv_message(Duration::from_secs(30)).unwrap();

        assert!(message.is_none());
        assert!(started_at.elapsed() < Duration::from_millis(100));
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
