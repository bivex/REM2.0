use crate::value_objects::{ByteRange, FilePath, FunctionName};
use serde::{Deserialize, Serialize};

/// A uniquely identified code fragment selected for extraction.
///
/// Invariants enforced at construction:
/// - `range` must be non-empty (`start < end`)
/// - `source_file` must be a valid, non-empty path string
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionTarget {
    /// Absolute path to the source file.
    pub source_file: FilePath,
    /// Byte range of the selected fragment within that file.
    pub range: ByteRange,
    /// Desired name for the extracted function.
    pub extracted_name: FunctionName,
}

impl ExtractionTarget {
    /// Constructs an `ExtractionTarget`, enforcing all domain invariants.
    pub fn new(
        source_file: FilePath,
        range: ByteRange,
        extracted_name: FunctionName,
    ) -> Result<Self, crate::errors::DomainError> {
        if range.is_empty() {
            return Err(crate::errors::DomainError::EmptySelectionRange);
        }
        Ok(Self { source_file, range, extracted_name })
    }
}

/// The outcome produced by the extraction pipeline.
///
/// On success the entity carries both the rewritten original function
/// (call-site replaced) and the full text of the newly extracted function.
/// On failure it carries a structured reason so the CLI can report it clearly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub target: ExtractionTarget,
    pub outcome: ExtractionOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExtractionOutcome {
    Success {
        /// Rewritten source of the original function (call site patched in).
        rewritten_caller: String,
        /// Full source text of the newly created extracted function.
        extracted_fn: String,
        /// `true` if a lifetime-repair loop was required.
        lifetime_repair_applied: bool,
        /// `true` if non-local control-flow encoding was required.
        control_flow_reified: bool,
    },
    Failure {
        reason: crate::errors::ExtractionFailure,
    },
}

/// Result of the optional equivalence verification pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub target: ExtractionTarget,
    pub verdict: VerificationVerdict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationVerdict {
    Proved,
    CounterexampleFound { detail: String },
    OutOfScope { reason: String },
    PipelineFailed { detail: String },
}
