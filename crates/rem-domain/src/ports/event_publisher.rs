use crate::events::{ExtractionEvent, VerificationEvent};

/// **Port**: event publishing — domain services emit events through this
/// interface; infrastructure wires it to structured logging, metrics, etc.
///
/// Implementors: `rem-infrastructure::adapters::event_publisher`.
pub trait ExtractionEventPublisher: Send + Sync {
    fn publish(&self, event: ExtractionEvent);
}

pub trait VerificationEventPublisher: Send + Sync {
    fn publish(&self, event: VerificationEvent);
}
