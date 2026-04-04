/// Errors that can occur within domain logic.
///
/// These represent violations of business rules, not infrastructure failures.
/// Infrastructure errors use `anyhow::Error` at the application boundary.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("invalid state transition: {from} → {to}")]
    InvalidStateTransition { from: String, to: &'static str },
}
