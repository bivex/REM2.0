/// rem-application — the use-case / application service layer.
///
/// This crate sits between the domain and the outside world.
/// It orchestrates domain services and ports to implement the two
/// top-level operations exposed by the CLI:
///
///   1. `ExtractFunctionUseCase`  — performs the full extraction pipeline.
///   2. `VerifyEquivalenceUseCase` — runs the optional proof-generation pass.
///
/// Application services:
///   - depend on `rem-domain` (ports + services + entities)
///   - NEVER depend on `rem-infrastructure` directly (only on trait objects)
///   - accept DTOs at their boundary and return DTOs / results

pub mod dto;
pub mod use_cases;
pub mod errors;
