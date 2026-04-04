// ============================================================================
// Ports — Inbound/Outbound Interfaces
//
// Traits that define what the application layer needs from infrastructure.
// The domain never imports these — ports are the boundary between
// application logic and I/O implementations.
//
// Inbound ports:  the gRPC service calls into the application layer.
// Outbound ports: the application layer calls into infra via these traits.
// ============================================================================

pub mod agent_dispatcher;
pub mod event_stream;
pub mod repository;
