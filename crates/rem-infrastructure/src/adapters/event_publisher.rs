/// Adapter: structured `tracing` → `ExtractionEventPublisher`
///
/// Translates domain events into structured log records.
/// Metrics / spans can be wired here without touching the domain.

use rem_domain::{
    events::{ExtractionEvent, VerificationEvent},
    ports::event_publisher::{ExtractionEventPublisher, VerificationEventPublisher},
};
use tracing::{debug, info, warn};

pub struct TracingEventPublisher;

impl ExtractionEventPublisher for TracingEventPublisher {
    fn publish(&self, event: ExtractionEvent) {
        match &event {
            ExtractionEvent::Started { target } =>
                info!(file = %target.source_file, name = %target.extracted_name, "extraction.started"),
            ExtractionEvent::AnalysisCompleted { free_variables_count, .. } =>
                debug!(free_vars = free_variables_count, "extraction.analysis_completed"),
            ExtractionEvent::ControlFlowReificationRequired { kinds, .. } =>
                info!(?kinds, "extraction.cf_reification_required"),
            ExtractionEvent::LifetimeRepairIteration { iteration, error_code, .. } =>
                debug!(iteration, code = %error_code, "extraction.repair_iteration"),
            ExtractionEvent::LifetimeRepairSucceeded { total_iterations, .. } =>
                info!(iterations = total_iterations, "extraction.repair_succeeded"),
            ExtractionEvent::ExtractionSucceeded { target } =>
                info!(name = %target.extracted_name, "extraction.succeeded"),
            ExtractionEvent::ExtractionFailed { reason, .. } =>
                warn!(error = %reason, "extraction.failed"),
        }
    }
}

impl VerificationEventPublisher for TracingEventPublisher {
    fn publish(&self, event: VerificationEvent) {
        match &event {
            VerificationEvent::CharonTranslationStarted { .. } =>
                info!("verification.charon_started"),
            VerificationEvent::AeneasProofGenerated { .. } =>
                info!("verification.aeneas_generated"),
            VerificationEvent::EquivalenceProved { .. } =>
                info!("verification.proved"),
            VerificationEvent::VerificationSkipped { reason, .. } =>
                info!(reason, "verification.skipped"),
            VerificationEvent::VerificationFailed { detail, .. } =>
                warn!(error = %detail, "verification.failed"),
        }
    }
}
