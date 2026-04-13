use crate::{
    errors::DomainError,
    value_objects::{ByteRange, ControlFlowKind, FilePath, OwnershipKind},
};

/// Rich semantic information about the selected code fragment, produced
/// by the code-analysis adapter (rust-analyzer backed).
#[derive(Debug, Clone)]
pub struct SelectionAnalysis {
    /// Variables defined outside the selection and used inside it,
    /// together with the required passing convention.
    pub free_variables: Vec<FreeVariable>,
    /// Variables defined inside the selection and used after it.
    /// These become return values (possibly as a tuple).
    pub output_variables: Vec<OutputVariable>,
    /// Non-local control-flow operations found inside the selection.
    pub control_flow_exits: Vec<ControlFlowKind>,
    /// Whether the selection is inside an `async` context.
    pub is_async: bool,
    /// Whether the selection is inside a `const fn` context.
    pub is_const: bool,
    /// Generic parameters from the enclosing function that are referenced
    /// inside the selection (must be forwarded to the extracted fn).
    pub referenced_generics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FreeVariable {
    pub name: String,
    pub ty: String, // textual type as rust-analyzer resolves it
    pub ownership: OwnershipKind,
}

#[derive(Debug, Clone)]
pub struct OutputVariable {
    pub name: String,
    pub ty: String,
}

/// **Port**: code analysis — answers semantic questions about a source file.
///
/// Implementors: `rem-infrastructure::adapters::rust_analyzer`.
pub trait CodeAnalysisPort: Send + Sync {
    /// Load (or reuse a cached) workspace rooted at `project_root`.
    fn load_workspace(&self, project_root: &str) -> Result<(), DomainError>;

    /// Analyse the selected byte range inside `file` and return the full
    /// semantic picture needed for extraction.
    fn analyse_selection(
        &self,
        file: &FilePath,
        range: ByteRange,
    ) -> Result<SelectionAnalysis, DomainError>;
}
