use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = serde_json::from_str(&line)?;
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            continue;
        };
        let id = request.get("id").cloned();
        match method {
            "initialize" => respond(
                &mut stdout,
                id,
                json!({
                    "protocolVersion": 1,
                    "agentCapabilities": {},
                    "agentInfo": { "name": "echo-acp-agent", "version": "test" },
                    "authMethods": []
                }),
            )?,
            "session/new" => respond(&mut stdout, id, json!({ "sessionId": "echo-session" }))?,
            "session/prompt" => {
                let text = request["params"]["prompt"]
                    .as_array()
                    .and_then(|blocks| blocks.first())
                    .and_then(|block| block.get("text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let session_id = request["params"]["sessionId"]
                    .as_str()
                    .unwrap_or("echo-session");
                write_json_line(
                    &mut stdout,
                    json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": session_id,
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": format!("echo: {text}") }
                            }
                        }
                    }),
                )?;
                respond(&mut stdout, id, json!({ "stopReason": "end_turn" }))?;
            }
            _ => {
                if let Some(id) = id {
                    write_json_line(
                        &mut stdout,
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32601, "message": format!("unknown method {method}") }
                        }),
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn respond(
    stdout: &mut impl Write,
    id: Option<Value>,
    result: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(id) = id {
        write_json_line(
            stdout,
            json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        )?;
    }
    Ok(())
}

fn write_json_line(
    stdout: &mut impl Write,
    value: Value,
) -> Result<(), Box<dyn std::error::Error>> {
    serde_json::to_writer(&mut *stdout, &value)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}
