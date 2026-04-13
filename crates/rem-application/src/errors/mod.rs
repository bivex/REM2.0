use thiserror::Error;

/// Errors that can be returned from the application layer to the CLI.
/// These are transport-level errors (missing ports, bad input).
/// Domain failures are surfaced inside the `ExtractFunctionResponse.error` field.
#[derive(Debug, Error)]
pub enum ApplicationError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("domain error: {0}")]
    Domain(#[from] rem_domain::errors::DomainError),

    #[error("infrastructure error: {0}")]
    Infrastructure(String),
}
