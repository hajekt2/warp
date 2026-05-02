use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC request id used by ACP. Warp-generated requests use numeric ids,
/// but agent-initiated requests may legally use strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcId {
    Number(u64),
    String(String),
}

impl From<u64> for JsonRpcId {
    fn from(value: u64) -> Self {
        Self::Number(value)
    }
}

impl fmt::Display for JsonRpcId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonRpcId::Number(value) => write!(f, "{value}"),
            JsonRpcId::String(value) => f.write_str(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcErrorObject {
    #[must_use]
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct JsonRpcRequest<'a, T> {
    pub jsonrpc: &'static str,
    pub id: JsonRpcId,
    pub method: &'a str,
    pub params: T,
}

impl<'a, T> JsonRpcRequest<'a, T> {
    #[must_use]
    pub(crate) fn new(id: JsonRpcId, method: &'a str, params: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method,
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct JsonRpcNotification<'a, T> {
    pub jsonrpc: &'static str,
    pub method: &'a str,
    pub params: T,
}

impl<'a, T> JsonRpcNotification<'a, T> {
    #[must_use]
    pub(crate) fn new(method: &'a str, params: T) -> Self {
        Self {
            jsonrpc: "2.0",
            method,
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct JsonRpcResult<T> {
    pub jsonrpc: &'static str,
    pub id: JsonRpcId,
    pub result: T,
}

impl<T> JsonRpcResult<T> {
    #[must_use]
    pub(crate) fn new(id: JsonRpcId, result: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct JsonRpcErrorResponse {
    pub jsonrpc: &'static str,
    pub id: JsonRpcId,
    pub error: JsonRpcErrorObject,
}

impl JsonRpcErrorResponse {
    #[must_use]
    pub(crate) fn new(id: JsonRpcId, error: JsonRpcErrorObject) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            error,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentMessage {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        id: JsonRpcId,
        method: String,
        params: Value,
    },
}

#[derive(Debug, PartialEq)]
pub(crate) enum IncomingFrame {
    Response {
        id: u64,
        result: Result<Value, JsonRpcErrorObject>,
    },
    AgentMessage(AgentMessage),
}

pub(crate) fn encode_frame<T: Serialize>(frame: &T) -> serde_json::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(frame)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn decode_frame(line: &str) -> serde_json::Result<IncomingFrame> {
    let value: Value = serde_json::from_str(line)?;
    let id = value.get("id").cloned();
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    match (id, method) {
        (Some(id_value), Some(method)) => {
            let id = serde_json::from_value(id_value)?;
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            Ok(IncomingFrame::AgentMessage(AgentMessage::Request {
                id,
                method,
                params,
            }))
        }
        (None, Some(method)) => {
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            Ok(IncomingFrame::AgentMessage(AgentMessage::Notification {
                method,
                params,
            }))
        }
        (Some(id_value), None) => {
            let JsonRpcId::Number(id) = serde_json::from_value(id_value)? else {
                return Err(<serde_json::Error as serde::de::Error>::custom(
                    "Warp request responses must use numeric request ids",
                ));
            };
            if let Some(error) = value.get("error") {
                Ok(IncomingFrame::Response {
                    id,
                    result: Err(serde_json::from_value(error.clone())?),
                })
            } else {
                Ok(IncomingFrame::Response {
                    id,
                    result: Ok(value.get("result").cloned().unwrap_or(Value::Null)),
                })
            }
        }
        (None, None) => Err(<serde_json::Error as serde::de::Error>::custom(
            "JSON-RPC frame must contain method or id",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_single_line_frame() {
        let frame = JsonRpcRequest::new(JsonRpcId::Number(1), "initialize", json!({}));
        let encoded = String::from_utf8(encode_frame(&frame).unwrap()).unwrap();

        assert!(encoded.ends_with('\n'));
        assert_eq!(encoded.matches('\n').count(), 1);
    }

    #[test]
    fn decodes_agent_notification() {
        let frame = decode_frame(r#"{"jsonrpc":"2.0","method":"session/update","params":{"x":1}}"#)
            .unwrap();

        assert_eq!(
            frame,
            IncomingFrame::AgentMessage(AgentMessage::Notification {
                method: "session/update".to_string(),
                params: json!({"x": 1}),
            })
        );
    }
}
