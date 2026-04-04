// ============================================================================
// gRPC Clients
//
// Thin wrappers around tonic-generated stubs. Cloned cheaply via Arc<Channel>.
// All clients share a single underlying connection pool per service.
// ============================================================================

mod session;

pub use session::SessionClient;
