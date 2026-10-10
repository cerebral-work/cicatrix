//! MCP server implementation supporting Stdio and HTTP transports.
//!
//! Dual transports:
//! - Stdio: line-delimited JSON-RPC 2.0 over standard I/O for local agent executions.
//! - HTTP: streaming HTTP 1.1 server supporting `GET /health`, `POST /mcp`, and `GET /sse`.

use std::io;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use serde_json::{json, Value};

use crate::mcp::protocol::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpToolCallResult, MCP_PROTOCOL_VERSION,
};
use crate::mcp::tools::{execute_tool, mcp_tools, ToolExecutionError};

/// Process a single JSON-RPC 2.0 request value, returning the JSON-RPC response value if not a notification.
pub async fn handle_json_rpc(req_val: Value) -> Option<Value> {
    let req: JsonRpcRequest = match serde_json::from_value(req_val) {
        Ok(r) => r,
        Err(e) => {
            return Some(
                serde_json::to_value(JsonRpcResponse::error(
                    Value::Null,
                    JsonRpcError::invalid_request(crate::masking::sanitize_all(&format!(
                        "invalid JSON-RPC request: {e}"
                    ))),
                ))
                .unwrap_or(Value::Null),
            );
        }
    };

    let id = req.id.clone();
    let is_notification = id.is_none();

    let res = match req.method.as_str() {
        "initialize" => Some(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "cicatrix",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "notifications/initialized" | "initialized" => None,
        "ping" => Some(json!({})),
        "tools/list" => {
            let tools = mcp_tools();
            Some(json!({ "tools": tools }))
        }
        "tools/call" => {
            let params = req.params.as_ref().and_then(Value::as_object);
            let tool_name = params.and_then(|p| p.get("name")).and_then(Value::as_str);
            let empty_args = json!({});
            let arguments = params
                .and_then(|p| p.get("arguments"))
                .unwrap_or(&empty_args);

            match tool_name {
                Some(name) => match execute_tool(name, arguments).await {
                    Ok(tool_res) => {
                        let mcp_res = McpToolCallResult::success_json(&tool_res)
                            .unwrap_or_else(|_| McpToolCallResult::success(tool_res.to_string()));
                        Some(serde_json::to_value(mcp_res).unwrap_or(Value::Null))
                    }
                    Err(ToolExecutionError::Client(msg)) => {
                        let mcp_res = McpToolCallResult::error(crate::masking::sanitize_all(&msg));
                        Some(serde_json::to_value(mcp_res).unwrap_or(Value::Null))
                    }
                    Err(ToolExecutionError::Internal(details)) => {
                        if let Some(req_id) = id {
                            let (err, _) = JsonRpcError::internal_masked(&details);
                            return Some(
                                serde_json::to_value(JsonRpcResponse::error(req_id, err))
                                    .unwrap_or(Value::Null),
                            );
                        } else {
                            return None;
                        }
                    }
                },
                None => {
                    if let Some(req_id) = id {
                        return Some(
                            serde_json::to_value(JsonRpcResponse::error(
                                req_id,
                                JsonRpcError::invalid_params(
                                    "tools/call requires `name` parameter",
                                ),
                            ))
                            .unwrap_or(Value::Null),
                        );
                    } else {
                        return None;
                    }
                }
            }
        }
        other => {
            if let Some(req_id) = id {
                return Some(
                    serde_json::to_value(JsonRpcResponse::error(
                        req_id,
                        JsonRpcError::method_not_found(other),
                    ))
                    .unwrap_or(Value::Null),
                );
            } else {
                return None;
            }
        }
    };

    if is_notification {
        return None;
    }

    let req_id = id.unwrap_or(Value::Null);
    res.map(|val| {
        serde_json::to_value(JsonRpcResponse::success(req_id, val)).unwrap_or(Value::Null)
    })
}

/// Run the stdio JSON-RPC MCP server until standard input closes.
pub async fn run_stdio_server() -> io::Result<()> {
    eprintln!("[cicatrix-mcp] starting stdio transport (MCP v{MCP_PROTOCOL_VERSION})...");
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parsed_val: Result<Value, _> = serde_json::from_str(trimmed);
        match parsed_val {
            Ok(req_val) => {
                if let Some(resp_val) = handle_json_rpc(req_val).await {
                    let mut resp_str = serde_json::to_string(&resp_val)?;
                    resp_str.push('\n');
                    stdout.write_all(resp_str.as_bytes()).await?;
                    stdout.flush().await?;
                }
            }
            Err(e) => {
                let err_resp = JsonRpcResponse::error(
                    Value::Null,
                    JsonRpcError::parse_error(format!("invalid JSON: {e}")),
                );
                let mut resp_str = serde_json::to_string(&err_resp)?;
                resp_str.push('\n');
                stdout.write_all(resp_str.as_bytes()).await?;
                stdout.flush().await?;
            }
        }
    }
    eprintln!("[cicatrix-mcp] stdio transport closed");
    Ok(())
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|w| w == b"\r\n\r\n")
}

async fn send_http_response(
    stream: &mut TcpStream,
    status_code: u16,
    reason: &str,
    content_type: &str,
    extra_headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let mut header_str = format!(
        "HTTP/1.1 {status_code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra_headers {
        header_str.push_str(&format!("{k}: {v}\r\n"));
    }
    header_str.push_str("\r\n");
    stream.write_all(header_str.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    let _ = stream.shutdown().await;
    Ok(())
}

async fn handle_rest_tool_call_value(
    tool_name: &str,
    payload: Value,
    stream: &mut TcpStream,
) -> io::Result<()> {
    match execute_tool(tool_name, &payload).await {
        Ok(res_val) => {
            let res_bytes = serde_json::to_vec(&res_val).unwrap_or_default();
            send_http_response(stream, 200, "OK", "application/json", &[], &res_bytes).await
        }
        Err(ToolExecutionError::Client(msg)) => {
            let masked = crate::masking::MaskedError::client(msg);
            let (status, resp_body, _) = masked.to_http_response();
            send_http_response(
                stream,
                status,
                "Bad Request",
                "application/json",
                &[],
                resp_body.as_bytes(),
            )
            .await
        }
        Err(ToolExecutionError::Internal(details)) => {
            let masked = crate::masking::MaskedError::internal(details);
            let (status, resp_body, correlation_id) = masked.to_http_response();
            let mut headers = Vec::new();
            if let Some(ref ref_id) = correlation_id {
                headers.push(("X-Correlation-Id", ref_id.as_str()));
            }
            send_http_response(
                stream,
                status,
                "Internal Server Error",
                "application/json",
                &headers,
                resp_body.as_bytes(),
            )
            .await
        }
    }
}

async fn handle_rest_tool_call(
    tool_name: &str,
    body: &[u8],
    stream: &mut TcpStream,
) -> io::Result<()> {
    let payload = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(e) => {
                let masked =
                    crate::masking::MaskedError::client(format!("invalid JSON payload: {e}"));
                let (status, resp_body, _) = masked.to_http_response();
                return send_http_response(
                    stream,
                    status,
                    "Bad Request",
                    "application/json",
                    &[],
                    resp_body.as_bytes(),
                )
                .await;
            }
        }
    };
    handle_rest_tool_call_value(tool_name, payload, stream).await
}

fn simple_url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '%' => {
                let h1 = chars.next();
                let h2 = chars.next();
                if let (Some(c1), Some(c2)) = (h1, h2) {
                    let hex = format!("{c1}{c2}");
                    if let Ok(val) = u8::from_str_radix(&hex, 16) {
                        result.push(val as char);
                        continue;
                    }
                    result.push('%');
                    result.push(c1);
                    result.push(c2);
                } else {
                    result.push('%');
                }
            }
            '+' => result.push(' '),
            other => result.push(other),
        }
    }
    result
}

fn parse_query_string(query: &str) -> Value {
    let mut map = serde_json::Map::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let k = simple_url_decode(k);
        let v = simple_url_decode(v);
        if k == "is_negative" {
            if let Ok(b) = v.parse::<bool>() {
                map.insert(k, Value::Bool(b));
                continue;
            }
        }
        if k == "limit" {
            if let Ok(num) = v.parse::<u64>() {
                map.insert(k, json!(num));
                continue;
            }
        }
        map.insert(k, Value::String(v));
    }
    Value::Object(map)
}

async fn handle_http_connection(mut stream: TcpStream) -> io::Result<()> {
    let mut buf = vec![0u8; 8192];
    let mut header_bytes = Vec::new();

    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        header_bytes.extend_from_slice(&buf[..n]);
        if let Some(pos) = find_header_end(&header_bytes) {
            let (headers_part, remaining_body) = header_bytes.split_at(pos);
            let headers_str = String::from_utf8_lossy(headers_part);
            let mut lines = headers_str.lines();
            let req_line = lines.next().unwrap_or("");
            let mut parts = req_line.split_whitespace();
            let method = parts.next().unwrap_or("").to_uppercase();
            let full_path = parts.next().unwrap_or("/");
            let (route, query) = full_path.split_once('?').unwrap_or((full_path, ""));

            let mut content_length: usize = 0;
            for line in lines {
                if let Some(val) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = val.trim().parse().unwrap_or(0);
                }
            }

            let initial_body = &remaining_body[4..];
            let mut body = initial_body.to_vec();
            while body.len() < content_length {
                let needed = content_length - body.len();
                let chunk_size = needed.min(buf.len());
                let read_n = stream.read(&mut buf[..chunk_size]).await?;
                if read_n == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..read_n]);
            }

            match (method.as_str(), route) {
                ("GET", "/health") => {
                    let res_body = json!({
                        "status": "ok",
                        "service": "cicatrix-mcp",
                        "version": env!("CARGO_PKG_VERSION"),
                    })
                    .to_string();
                    send_http_response(
                        &mut stream,
                        200,
                        "OK",
                        "application/json",
                        &[],
                        res_body.as_bytes(),
                    )
                    .await?;
                    return Ok(());
                }
                ("GET", "/sse") => {
                    let sse_header = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\n";
                    stream.write_all(sse_header.as_bytes()).await?;
                    let initial_event = "event: endpoint\r\ndata: /mcp\r\n\r\n";
                    stream.write_all(initial_event.as_bytes()).await?;
                    stream.flush().await?;
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    let _ = stream.shutdown().await;
                    return Ok(());
                }
                ("POST", "/mcp") | ("POST", "/") => {
                    let req_json: Result<Value, _> = serde_json::from_slice(&body);
                    match req_json {
                        Ok(val) => {
                            let resp = handle_json_rpc(val).await;
                            let res_body = match resp {
                                Some(r) => {
                                    serde_json::to_string(&r).unwrap_or_else(|_| "{}".to_string())
                                }
                                None => "{}".to_string(),
                            };
                            send_http_response(
                                &mut stream,
                                200,
                                "OK",
                                "application/json",
                                &[],
                                res_body.as_bytes(),
                            )
                            .await?;
                            return Ok(());
                        }
                        Err(e) => {
                            let sanitized =
                                crate::masking::sanitize_all(&format!("parse error: {e}"));
                            let err_body = json!({
                                "jsonrpc": "2.0",
                                "id": null,
                                "error": {
                                    "code": crate::mcp::protocol::PARSE_ERROR,
                                    "message": sanitized
                                }
                            })
                            .to_string();
                            send_http_response(
                                &mut stream,
                                400,
                                "Bad Request",
                                "application/json",
                                &[],
                                err_body.as_bytes(),
                            )
                            .await?;
                            return Ok(());
                        }
                    }
                }
                // REST API Endpoints with Unified Error Masking
                ("POST", "/api/v1/query") => {
                    handle_rest_tool_call("cicatrix_query_known_bugs", &body, &mut stream).await?;
                    return Ok(());
                }
                ("POST", "/api/v1/verify_diff") => {
                    handle_rest_tool_call("cicatrix_verify_diff", &body, &mut stream).await?;
                    return Ok(());
                }
                ("POST", "/api/v1/reversibility") => {
                    handle_rest_tool_call("cicatrix_verify_reversibility", &body, &mut stream)
                        .await?;
                    return Ok(());
                }
                ("POST", "/api/v1/tripwire/check") => {
                    handle_rest_tool_call("cicatrix_check_tripwire", &body, &mut stream).await?;
                    return Ok(());
                }
                ("POST", "/api/v1/autonomy/tier") => {
                    handle_rest_tool_call("cicatrix_get_autonomy_tier", &body, &mut stream).await?;
                    return Ok(());
                }
                ("POST", "/api/v1/autonomy/record") => {
                    handle_rest_tool_call("cicatrix_record_autonomy_event", &body, &mut stream)
                        .await?;
                    return Ok(());
                }
                ("POST", "/api/v1/autonomy/history") => {
                    handle_rest_tool_call("cicatrix_list_autonomy_history", &body, &mut stream)
                        .await?;
                    return Ok(());
                }
                ("POST", "/api/v1/autonomy/check") => {
                    handle_rest_tool_call("cicatrix_check_autonomy", &body, &mut stream).await?;
                    return Ok(());
                }
                ("POST", "/api/v1/context/assemble") => {
                    handle_rest_tool_call("cicatrix_assemble_run_context", &body, &mut stream)
                        .await?;
                    return Ok(());
                }
                ("POST", "/api/v1/cortex/settle") => {
                    handle_rest_tool_call("cicatrix_ingest_cortex_settle", &body, &mut stream)
                        .await?;
                    return Ok(());
                }
                ("GET", "/api/v1/cortex/settles") => {
                    handle_rest_tool_call_value(
                        "cicatrix_list_cortex_settles",
                        parse_query_string(query),
                        &mut stream,
                    )
                    .await?;
                    return Ok(());
                }
                ("POST", "/api/v1/cortex/settles") => {
                    handle_rest_tool_call("cicatrix_list_cortex_settles", &body, &mut stream)
                        .await?;
                    return Ok(());
                }
                ("GET", "/api/v1/cortex/settle/status") => {
                    handle_rest_tool_call_value(
                        "cicatrix_cortex_settle_status",
                        parse_query_string(query),
                        &mut stream,
                    )
                    .await?;
                    return Ok(());
                }
                ("POST", "/api/v1/cortex/settle/status") => {
                    handle_rest_tool_call("cicatrix_cortex_settle_status", &body, &mut stream)
                        .await?;
                    return Ok(());
                }
                ("GET", "/api/v1/error/simulate_500") | ("POST", "/api/v1/error/simulate_500") => {
                    let masked = crate::masking::MaskedError::internal(
                        "simulated internal database error at /home/ctodie/db.sqlite with token=secret123"
                    );
                    let (status, resp_body, correlation_id) = masked.to_http_response();
                    let mut headers = Vec::new();
                    if let Some(ref ref_id) = correlation_id {
                        headers.push(("X-Correlation-Id", ref_id.as_str()));
                    }
                    send_http_response(
                        &mut stream,
                        status,
                        "Internal Server Error",
                        "application/json",
                        &headers,
                        resp_body.as_bytes(),
                    )
                    .await?;
                    return Ok(());
                }
                _ => {
                    let not_found =
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    stream.write_all(not_found.as_bytes()).await?;
                    stream.flush().await?;
                    let _ = stream.shutdown().await;
                    return Ok(());
                }
            }
        }
        if header_bytes.len() > 65536 {
            let too_large = "HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(too_large.as_bytes()).await?;
            stream.flush().await?;
            let _ = stream.shutdown().await;
            return Ok(());
        }
    }
}

/// Run the streaming HTTP MCP server listening on `bind_addr`.
pub async fn run_http_server(bind_addr: &str) -> io::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    eprintln!(
        "[cicatrix-mcp] HTTP server listening on http://{bind_addr} (/health, /mcp, /sse)..."
    );

    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_http_connection(socket).await {
                eprintln!("[cicatrix-mcp] connection error: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_jsonrpc_initialize_and_tools_list() {
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let init_resp = handle_json_rpc(init_req).await.expect("response expected");
        assert_eq!(init_resp["id"], 1);
        assert_eq!(init_resp["result"]["protocolVersion"], MCP_PROTOCOL_VERSION);
        assert_eq!(init_resp["result"]["serverInfo"]["name"], "cicatrix");

        let list_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        });
        let list_resp = handle_json_rpc(list_req).await.expect("response expected");
        assert_eq!(list_resp["id"], 2);
        let tools = list_resp["result"]["tools"]
            .as_array()
            .expect("array of tools");
        assert!(tools.len() >= 6);
    }

    #[tokio::test]
    async fn test_jsonrpc_unknown_method() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "unknown_tool_rpc"
        });
        let resp = handle_json_rpc(req).await.expect("response expected");
        assert_eq!(resp["id"], 99);
        assert_eq!(
            resp["error"]["code"],
            crate::mcp::protocol::METHOD_NOT_FOUND
        );
    }

    #[tokio::test]
    async fn test_http_cortex_settle_endpoints() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("cortex_test.db");
        let db_path_str = db_path.to_str().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let sessions_dir_str = sessions_dir.to_str().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let _ = handle_http_connection(socket).await;
                });
            }
        });

        // 1. Ingest negative settle event via POST /api/v1/cortex/settle
        let mut client = TcpStream::connect(local_addr).await.unwrap();
        let payload = json!({
            "event_id": "evt-http-001",
            "job_id": "job-http-001",
            "settle_action": "discard",
            "verdict": "operator discarded run",
            "db_path": db_path_str,
            "observed_dir": sessions_dir_str,
        });
        let body = serde_json::to_vec(&payload).unwrap();
        let req = format!(
            "POST /api/v1/cortex/settle HTTP/1.1\r\nHost: {local_addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        client.write_all(req.as_bytes()).await.unwrap();
        client.write_all(&body).await.unwrap();

        let mut res = Vec::new();
        client.read_to_end(&mut res).await.unwrap();
        let res_str = String::from_utf8_lossy(&res);
        assert!(
            res_str.contains("200 OK"),
            "Expected 200 OK, got: {res_str}"
        );
        assert!(res_str.contains("\"status\":\"ingested_negative\""));
        assert!(res_str.contains("\"is_negative\":true"));

        // 2. Query status via GET /api/v1/cortex/settle/status?db_path=...
        let mut client2 = TcpStream::connect(local_addr).await.unwrap();
        let req2 = format!(
            "GET /api/v1/cortex/settle/status?db_path={db_path_str} HTTP/1.1\r\nHost: {local_addr}\r\nConnection: close\r\n\r\n"
        );
        client2.write_all(req2.as_bytes()).await.unwrap();
        let mut res2 = Vec::new();
        client2.read_to_end(&mut res2).await.unwrap();
        let res2_str = String::from_utf8_lossy(&res2);
        assert!(
            res2_str.contains("200 OK"),
            "Expected 200 OK, got: {res2_str}"
        );
        assert!(
            res2_str.contains("\"negative_verdicts\":1"),
            "Expected negative_verdicts:1, got: {res2_str}"
        );

        // 3. Query settles via GET /api/v1/cortex/settles?db_path=...&is_negative=true
        let mut client3 = TcpStream::connect(local_addr).await.unwrap();
        let req3 = format!(
            "GET /api/v1/cortex/settles?db_path={db_path_str}&is_negative=true HTTP/1.1\r\nHost: {local_addr}\r\nConnection: close\r\n\r\n"
        );
        client3.write_all(req3.as_bytes()).await.unwrap();
        let mut res3 = Vec::new();
        client3.read_to_end(&mut res3).await.unwrap();
        let res3_str = String::from_utf8_lossy(&res3);
        assert!(
            res3_str.contains("200 OK"),
            "Expected 200 OK, got: {res3_str}"
        );
        assert!(
            res3_str.contains("evt-http-001"),
            "Expected evt-http-001, got: {res3_str}"
        );
    }
}
