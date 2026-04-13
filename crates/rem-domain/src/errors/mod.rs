use thiserror::Error;

/// All errors that can originate inside the domain layer.
///
/// Infrastructure errors (I/O, compiler invocation, etc.) are expressed as
/// separate error types in their own crates and converted to these variants
/// only when they cross a domain port boundary.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DomainError {
    // ── Value-object construction ─────────────────────────────────────────
    #[error("selection range is empty or inverted")]
    EmptySelectionRange,

    #[error("invalid file path: `{0}`")]
    InvalidFilePath(String),

    #[error("invalid function name: `{0}` is not a legal Rust identifier")]
    InvalidFunctionName(String),

    #[error("invalid lifetime parameter: `{0}` (must start with `'`)")]
    InvalidLifetimeParameter(String),

    // ── Extraction pre-conditions ─────────────────────────────────────────
    #[error("selected range does not form a syntactically complete statement list")]
    IncompleteStatementList,

    #[error("selected range spans multiple functions")]
    MultipleEnclosingFunctions,

    #[error("cannot extract: selection contains an unsupported Rust construct ({0})")]
    UnsupportedConstruct(String),
}

/// Structured reasons why an extraction attempt failed.
/// Kept separate from `DomainError` so callers can match on the success
/// path independently.
#[derive(Debug, Clone, Error, serde::Serialize, serde::Deserialize)]
pub enum ExtractionFailure {
    #[error("pre-flight: {0}")]
    PreFlight(String),

    #[error("lifetime repair exhausted after {iterations} iterations: {last_error}")]
    LifetimeRepairExhausted { iterations: u32, last_error: String },

    #[error("cannot reify control flow: {0}")]
    ControlFlowReificationFailed(String),

    #[error("rust-analyzer analysis failed: {0}")]
    AnalysisFailed(String),

    #[error("cargo check failed after extraction: {0}")]
    CargoCheckFailed(String),
}
