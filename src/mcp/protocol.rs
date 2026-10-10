//! Model Context Protocol (MCP) and JSON-RPC 2.0 wire definitions.
//!
//! Conforms to MCP 2024-11-05 and JSON-RPC 2.0 specifications with
//! Wheelhorse-governed correlation ID masking for internal 5xx errors.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC_VERSION: &str = "2.0";
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

pub use crate::masking::generate_correlation_id;

/// Incoming JSON-RPC 2.0 Request or Notification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// Outgoing JSON-RPC 2.0 Response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 Error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(INVALID_PARAMS, message)
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(METHOD_NOT_FOUND, format!("method `{method}` not found"))
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(INVALID_REQUEST, message)
    }

    pub fn parse_error(message: impl Into<String>) -> Self {
        Self::new(PARSE_ERROR, message)
    }

    /// Masked 5xx internal server error returning generic message with a `ref_` correlation ID.
    pub fn internal_masked(err_details: &str) -> (Self, String) {
        let masked = crate::masking::MaskedError::internal(err_details);
        let ref_id = masked
            .correlation_id
            .unwrap_or_else(crate::masking::generate_correlation_id);
        (
            Self {
                code: INTERNAL_ERROR,
                message: format!("internal server error (correlation: {ref_id})"),
                data: None,
            },
            ref_id,
        )
    }
}

/// MCP Tool definition for `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// MCP Content payload for tool call results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// MCP Tool call execution result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpToolCallResult {
    pub content: Vec<McpContent>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl McpToolCallResult {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            content: vec![McpContent {
                content_type: "text".to_string(),
                text: text.into(),
            }],
            is_error: None,
        }
    }

    pub fn success_json<T: Serialize>(data: &T) -> Result<Self, serde_json::Error> {
        let text = serde_json::to_string_pretty(data)?;
        Ok(Self::success(text))
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![McpContent {
                content_type: "text".to_string(),
                text: text.into(),
            }],
            is_error: Some(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correlation_id_prefix_and_uniqueness() {
        let id1 = generate_correlation_id();
        let id2 = generate_correlation_id();
        assert!(id1.starts_with("ref_"));
        assert!(id2.starts_with("ref_"));
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_jsonrpc_response_serialization() {
        let res = JsonRpcResponse::success(Value::from(1), serde_json::json!({"status": "ok"}));
        let serialized = serde_json::to_string(&res).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"id\":1"));
        assert!(serialized.contains("\"result\":{\"status\":\"ok\"}"));
    }

    #[test]
    fn test_masked_error_format() {
        let (err, ref_id) = JsonRpcError::internal_masked("database disk full");
        assert_eq!(err.code, INTERNAL_ERROR);
        assert!(err
            .message
            .contains("internal server error (correlation: ref_"));
        assert!(err.message.contains(&ref_id));
    }
}
