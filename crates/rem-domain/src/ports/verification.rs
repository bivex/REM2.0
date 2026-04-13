use crate::{entities::VerificationResult, errors::DomainError, value_objects::FilePath};

/// **Port**: equivalence verification — translates original + extracted
/// functions via CHARON → AENEAS → Coq and checks the produced proof.
///
/// This is an optional, opt-in pipeline step.
/// Implementors: `rem-infrastructure::adapters::charon_aeneas`.
pub trait VerificationPort: Send + Sync {
    /// Verify that `original_fn` and `extracted_fn` inside `source_file`
    /// are behaviourally equivalent for the supported Rust subset.
    ///
    /// Returns `VerificationResult` even on soft failures (out-of-scope,
    /// etc.); only hard infrastructure errors become `Err`.
    fn verify_equivalence(
        &self,
        source_file: &FilePath,
        original_fn_name: &str,
        extracted_fn_name: &str,
        project_root: &str,
    ) -> Result<VerificationResult, DomainError>;
}
