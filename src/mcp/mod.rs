pub mod client;
pub mod server;
pub use server::MCPServer;

// gRPC server is kept behind the feature flag for future v1.1 re-introduction.
// The client stub was removed in v1.0 — use MCPTransport::Http instead.
#[cfg(feature = "tools-mcp-grpc")]
pub mod grpc;
