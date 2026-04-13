/// rem-domain — the innermost layer.
///
/// Nothing in this crate may depend on rust-analyzer, the file system,
/// the compiler, or any other piece of infrastructure. All dependencies
/// on the outside world are expressed as traits (ports) that live here
/// and are implemented in `rem-infrastructure`.
///
/// Module layout
/// ─────────────
///  entities/          — identifiable, mutable domain objects (by ID)
///  value_objects/     — immutable, equality-by-value building blocks
///  ports/             — trait interfaces that domain/application depend on
///  services/          — stateless, pure domain logic
///  events/            — things that happened inside the domain
///  errors/            — typed, domain-level error taxonomy

pub mod entities;
pub mod value_objects;
pub mod ports;
pub mod services;
pub mod events;
pub mod errors;
