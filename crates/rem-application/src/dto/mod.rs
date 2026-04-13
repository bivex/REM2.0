use serde::{Deserialize, Serialize};

// ── Input DTOs ────────────────────────────────────────────────────────────────

/// Everything the CLI passes to `ExtractFunctionUseCase`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractFunctionRequest {
    /// Absolute path to the source file containing the selection.
    pub file: String,
    /// Start byte offset (inclusive) of the selected fragment.
    pub start_byte: u32,
    /// End byte offset (exclusive) of the selected fragment.
    pub end_byte: u32,
    /// Desired name for the new extracted function.
    pub extracted_fn_name: String,
    /// Workspace / project root (directory containing `Cargo.toml`).
    /// If `None`, it is auto-detected from `file`.
    pub project_root: Option<String>,
    /// Whether to run the optional CHARON/AENEAS verification pass.
    pub verify: bool,
}

/// Everything the CLI passes to `VerifyEquivalenceUseCase`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyEquivalenceRequest {
    pub file: String,
    pub original_fn_name: String,
    pub extracted_fn_name: String,
    pub project_root: String,
}

// ── Output DTOs ───────────────────────────────────────────────────────────────

/// Result returned to the CLI after an extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractFunctionResponse {
    pub success: bool,
    /// Populated on success: new content of the source file.
    pub new_file_content: Option<String>,
    /// Human-readable description of what was done.  Always present.
    pub summary: String,
    /// Populated on failure.
    pub error: Option<String>,
    pub stats: ExtractionStats,
    /// Present if `verify = true` was requested.
    pub verification: Option<VerificationResponse>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionStats {
    pub lifetime_repair_iterations: u32,
    pub control_flow_reified: bool,
    pub is_async: bool,
    pub is_const: bool,
}

/// Result returned to the CLI after a verification run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResponse {
    pub verdict: String,        // "proved" | "counterexample" | "out_of_scope" | "failed"
    pub detail: Option<String>,
}
