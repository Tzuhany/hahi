// ============================================================================
// Domain — Pure Business Types
//
// No I/O, no async, no framework imports. Just Rust types and rules.
//
// Everything in this module can be reasoned about and tested in isolation.
// The rest of the service exists to persist and serve these types.
// ============================================================================

pub mod error;
pub mod ids;
pub mod message;
pub mod run;
pub mod thread;

pub use ids::{MessageId, RunId, ThreadId};
pub use message::{Message, MessageRole};
pub use run::{Run, RunStatus};
pub use thread::Thread;
