//! Native Model Context Protocol (MCP) provider interface (CER-2758, Phase 2.3).
//!
//! Exposes dual transports:
//! - Stdio transport for local agents (`cicatrix mcp` / `cicatrix mcp --stdio`).
//! - Streaming HTTP transport for cluster runners (`cicatrix mcp --http <bind>` / `cicatrix serve --mcp [--bind <bind>]`).
//!
//! Provides tools:
//! - `cicatrix_query_known_bugs`
//! - `cicatrix_verify_diff`
//! - `cicatrix_record_defect`
//! - `cicatrix_start_workflow`
//! - `cicatrix_workflow_status`
//! - `cicatrix_submit_signal`

pub mod protocol;
pub mod server;
pub mod tools;

pub use protocol::{
    generate_correlation_id, JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpContent,
    McpToolCallResult, McpToolDefinition, MCP_PROTOCOL_VERSION,
};
pub use server::{handle_json_rpc, run_http_server, run_stdio_server};
pub use tools::{execute_tool, mcp_tools, ToolExecutionError};
