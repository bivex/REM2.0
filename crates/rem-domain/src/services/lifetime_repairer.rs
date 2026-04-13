use crate::{
    errors::ExtractionFailure,
    ports::repair::{LifetimeRepairPort, RepairOutcome},
};

/// Maximum number of repair iterations before giving up.
pub const MAX_REPAIR_ITERATIONS: u32 = 12;

/// Pure orchestration service: drives the lifetime-repair feedback loop.
///
/// Given an initial source patch it calls the compiler through
/// `LifetimeRepairPort`, inspects diagnostics, mutates the signature,
/// and loops until the code compiles or the iteration budget is exhausted.
///
/// All mutable state is local to `repair()` — the service itself is
/// stateless and trivially testable with a stub port.
pub struct LifetimeRepairer;

impl LifetimeRepairer {
    /// Run the repair loop.
    ///
    /// `project_root`  — workspace root passed to the port.
    /// `initial_patch` — source text with the naive extraction already applied.
    /// `port`          — compiler-check adapter (injected).
    /// `on_iteration`  — callback invoked after each iteration (for event emission).
    pub fn repair<F>(
        project_root: &str,
        file: &crate::value_objects::FilePath,
        initial_patch: String,
        port: &dyn LifetimeRepairPort,
        mut on_iteration: F,
    ) -> Result<String, ExtractionFailure>
    where
        F: FnMut(u32, &str),
    {
        let mut current = initial_patch;

        for iteration in 1..=MAX_REPAIR_ITERATIONS {
            let outcome = port
                .check(project_root, file, &current)
                .map_err(|e| ExtractionFailure::AnalysisFailed(e.to_string()))?;

            match outcome {
                RepairOutcome::Accepted => return Ok(current),
                RepairOutcome::Rejected { diagnostics } => {
                    // Pick the first actionable lifetime error.
                    let first = diagnostics
                        .iter()
                        .find(|d| d.error_code.starts_with("E0"))
                        .ok_or_else(|| ExtractionFailure::LifetimeRepairExhausted {
                            iterations: iteration,
                            last_error: "no actionable diagnostic found".into(),
                        })?;

                    on_iteration(iteration, &first.error_code);
                    
                    current = apply_smarter_repair(&current, first, iteration);
                }
            }
        }

        Err(ExtractionFailure::LifetimeRepairExhausted {
            iterations: MAX_REPAIR_ITERATIONS,
            last_error: "budget exhausted".into(),
        })
    }
}

fn apply_smarter_repair(src: &str, diagnostic: &crate::ports::repair::CompilerDiagnostic, n: u32) -> String {
    let new_src = src.to_string();
    
    match diagnostic.error_code.as_str() {
        "E0106" => {
            // Missing lifetime. If the message mentions a parameter, try to use it.
            // Simplified: just add a named lifetime to the signature.
            apply_lifetime_heuristic(new_src, n)
        }
        "E0369" | "E0368" => {
            // Binary operation / Assignment on references. 
            // Try to inject dereference operator '*' if the span mentions one of our variables.
            if let Some(span) = &diagnostic.span_text {
                // This is a bit hacky but works for the demo: 
                // find the first occurrence of the span in src and prepend '*' to variables.
                // In a real tool, we would use RA to find the exact Expr and rewrite it.
                if let Some(_pos) = src.find(span) {
                    let mut patched_span = span.clone();
                    // If it's something like "accumulator += multiplier", 
                    // we want "*accumulator += *multiplier" (if both are refs).
                    // For now, let's just try to be slightly smarter.
                    // We'll replace the first occurrence of the span with a deref'd version.
                    // (Very aggressive heuristic for TDD).
                    
                    // Simple regex-like replacement for variables.
                    // Since we don't have the variable list here easily, 
                    // we'll just try to prepend '*' to words that are likely variables.
                    let words: Vec<&str> = span.split_whitespace().collect();
                    for word in words {
                        if word.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
                             if !["for", "in", "if", "while", "return", "matches"].contains(&word) {
                                 patched_span = patched_span.replace(word, &format!("*{}", word));
                             }
                        }
                    }
                    return src.replacen(span, &patched_span, 1);
                }
            }
            apply_lifetime_heuristic(new_src, n)
        }
        _ => apply_lifetime_heuristic(new_src, n)
    }
}

/// Append a fresh lifetime parameter `'remN` to the first `fn <` or `fn name(`
/// in the source patch.  A real repair loop narrows down the exact span from
/// the compiler diagnostic; this heuristic is the fallback.
fn apply_lifetime_heuristic(src: String, n: u32) -> String {
    let lt = format!("'rem{n}");
    // Try to inject into an existing generic list `fn foo<`.
    if let Some(pos) = src.find("fn ").and_then(|p| src[p..].find('<').map(|q| p + q + 1)) {
        let (before, after) = src.split_at(pos);
        return format!("{before}{lt}, {after}");
    }
    // Otherwise inject a new generic parameter: `fn foo(` → `fn foo<'remN>(`
    if let Some(pos) = src.find("fn ").and_then(|p| src[p..].find('(').map(|q| p + q)) {
        let (before, after) = src.split_at(pos);
        return format!("{before}<{lt}>{after}");
    }
    // Fallback: return unchanged (the loop will exhaust).
    src
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        errors::DomainError,
        ports::repair::{CompilerDiagnostic, LifetimeRepairPort, RepairOutcome},
    };

    struct AlwaysAccepts;
    impl LifetimeRepairPort for AlwaysAccepts {
        fn check(&self, _: &str, _: &crate::value_objects::FilePath, _: &str) -> Result<RepairOutcome, DomainError> {
            Ok(RepairOutcome::Accepted)
        }
    }

    struct AcceptsAfter(std::sync::atomic::AtomicU32);
    impl LifetimeRepairPort for AcceptsAfter {
        fn check(&self, _: &str, _: &crate::value_objects::FilePath, _: &str) -> Result<RepairOutcome, DomainError> {
            let n = self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if n < 2 {
                Ok(RepairOutcome::Rejected {
                    diagnostics: vec![CompilerDiagnostic {
                        error_code: "E0106".into(),
                        message: "missing lifetime".into(),
                        span_text: None,
                    }],
                })
            } else {
                Ok(RepairOutcome::Accepted)
            }
        }
    }

    #[test]
    fn accepts_immediately() {
        let dummy_file = crate::value_objects::FilePath::new("/dummy").unwrap();
        let result = LifetimeRepairer::repair("/", &dummy_file, "fn foo() {}".into(), &AlwaysAccepts, |_, _| {});
        assert!(result.is_ok());
    }

    #[test]
    fn accepts_after_two_rejections() {
        let dummy_file = crate::value_objects::FilePath::new("/dummy").unwrap();
        let port = AcceptsAfter(std::sync::atomic::AtomicU32::new(0));
        let result =
            LifetimeRepairer::repair("/", &dummy_file, "fn foo() {}".into(), &port, |_, _| {});
        assert!(result.is_ok());
    }
}
