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
            tracing::info!(iteration, current, "repair iteration");
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
    match diagnostic.error_code.as_str() {
        "E0106" => {
            // Missing lifetime.
            // Try to extract the parameter name from the message if possible.
            // Example: "missing lifetime specifier... help: this function's return type contains a borrowed value, 
            // but the signature does not say whether it is borrowed from `accumulator` or `multiplier`"
            let msg = &diagnostic.message;
            let mut target_name = None;
            
            // Heuristic: find the first backticked word in the message that isn't 'static or a type.
            for word in msg.split('`').step_by(2).skip(1) {
                if !["static", "i32", "u32", "str", "String", "Self"].contains(&word) {
                    target_name = Some(word);
                    break;
                }
            }

            if let Some(target) = target_name {
                // Try to find the parameter in the source and add a lifetime to its type.
                // e.g. "accumulator: &mut i32" -> "accumulator: &'remN mut i32"
                let lt = format!("'rem{}", n);
                let search_pattern = format!("{}: &", target);
                if let Some(pos) = src.find(&search_pattern) {
                    let insert_pos = pos + search_pattern.len();
                    let (before, after) = src.split_at(insert_pos);
                    // Special case for &mut
                    if after.starts_with("mut ") {
                        return format!("{}{} {}", before, lt, after);
                    } else {
                        return format!("{}{} {}", before, lt, after);
                    }
                }
            }
            
            apply_lifetime_heuristic(src.to_string(), n)
        }
        "E0369" | "E0368" => {
            // Binary operation / Assignment on references. 
            // The Deref Rewriter in CodeGenerator usually handles this, 
            // but if we are here, it means it missed something or a new error appeared.
            if let Some(span) = &diagnostic.span_text {
                if let Some(_pos) = src.find(span) {
                    let mut patched_span = span.clone();
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
            apply_lifetime_heuristic(src.to_string(), n)
        }
        _ => apply_lifetime_heuristic(src.to_string(), n)
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
