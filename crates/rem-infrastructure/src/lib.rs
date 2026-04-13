/// rem-infrastructure — all adapters that implement domain ports.
///
/// Each adapter is a concrete struct that implements exactly one port trait
/// from `rem-domain::ports`.  No business logic lives here.
///
/// adapters/
///   rust_analyzer  — implements `CodeAnalysisPort`
///   cargo          — implements `LifetimeRepairPort`
///   filesystem     — implements `FileSystemPort`
///   charon_aeneas  — implements `VerificationPort`
///   event_publisher — implements `ExtractionEventPublisher` / `VerificationEventPublisher`

pub mod adapters;
