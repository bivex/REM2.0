use crate::errors::DomainError;

/// Result of one lifetime-repair iteration.
#[derive(Debug, Clone)]
pub enum RepairOutcome {
    /// The code now compiles; no more iterations needed.
    Accepted,
    /// The compiler produced errors; the text contains structured diagnostics
    /// that the repair loop will use to decide the next transformation.
    Rejected { diagnostics: Vec<CompilerDiagnostic> },
}

#[derive(Debug, Clone)]
pub struct CompilerDiagnostic {
    pub error_code: String,
    pub message: String,
    pub span_text: Option<String>,
    /// Help/note messages from child diagnostics (e.g. "consider adding `'a`")
    pub help_text: Option<String>,
}

/// **Port**: lifetime & borrow repair — checks compiler acceptance and
/// derives the next mutation to apply to the extracted function signature.
///
/// Implementors: `rem-infrastructure::adapters::cargo`.
pub trait LifetimeRepairPort: Send + Sync {
    /// Run `cargo check` (or equivalent in-process check) on the project
    /// rooted at `project_root` after the given `source_patch` has been
    /// applied, and return the diagnostic outcome.
    fn check(
        &self,
        project_root: &str,
        file: &crate::value_objects::FilePath,
        source_patch: &str,
    ) -> Result<RepairOutcome, DomainError>;
}
