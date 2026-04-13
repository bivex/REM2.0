/// Ports — trait interfaces that isolate domain/application from all
/// infrastructure details (rust-analyzer, file system, cargo, CHARON/AENEAS).
///
/// Every port lives here. Implementations live in `rem-infrastructure`.
/// The domain and application layers depend ONLY on these traits, never
/// on concrete types from external libraries.

pub mod analysis;
pub mod repair;
pub mod verification;
pub mod filesystem;
pub mod event_publisher;
