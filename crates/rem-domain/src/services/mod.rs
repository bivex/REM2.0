/// Domain services — stateless, pure business logic.
///
/// Services coordinate value objects and ports without touching I/O
/// directly.  They are the only place where extraction / control-flow /
/// lifetime-repair algorithms are expressed.

pub mod ownership_oracle;
pub mod control_flow_analyzer;
pub mod lifetime_repairer;
pub mod code_generator;
