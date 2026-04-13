use serde::{Deserialize, Serialize};
use crate::{entities::ExtractionTarget, errors::ExtractionFailure};

/// Domain events record significant things that happened during a pipeline run.
/// They are emitted by domain services and consumed by application-layer
/// handlers (logging, metrics, etc.).  No side-effects happen inside the domain.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExtractionEvent {
    /// The extraction pipeline was successfully invoked with a valid target.
    Started { target: ExtractionTarget },

    /// rust-analyzer completed semantic analysis of the selection.
    AnalysisCompleted {
        target: ExtractionTarget,
        free_variables_count: usize,
    },

    /// Non-local control flow was detected and is being reified.
    ControlFlowReificationRequired {
        target: ExtractionTarget,
        kinds: Vec<crate::value_objects::ControlFlowKind>,
    },

    /// The lifetime-repair loop ran one more iteration.
    LifetimeRepairIteration {
        target: ExtractionTarget,
        iteration: u32,
        error_code: String,
    },

    /// All repairs applied; the borrow checker accepts the result.
    LifetimeRepairSucceeded {
        target: ExtractionTarget,
        total_iterations: u32,
    },

    /// The extraction completed successfully.
    ExtractionSucceeded { target: ExtractionTarget },

    /// The extraction failed with a structured reason.
    ExtractionFailed {
        target: ExtractionTarget,
        reason: ExtractionFailure,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VerificationEvent {
    /// The CHARON translation step started.
    CharonTranslationStarted { target: ExtractionTarget },

    /// AENEAS generated a Coq proof obligation.
    AeneasProofGenerated { target: ExtractionTarget },

    /// Coq checked the proof successfully.
    EquivalenceProved { target: ExtractionTarget },

    /// Verification was skipped because the fragment is outside the
    /// supported subset (e.g., uses `dyn Trait`).
    VerificationSkipped { target: ExtractionTarget, reason: String },

    /// The verification pipeline failed for an infrastructure reason.
    VerificationFailed { target: ExtractionTarget, detail: String },
}
