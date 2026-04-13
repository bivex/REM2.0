/// Adapter: CHARON + AENEAS → `VerificationPort`
///
/// Invokes the external CHARON binary to translate Rust → LLBC/MIR JSON,
/// then AENEAS to generate a Coq proof, and finally checks the proof.
///
/// Verification is intentionally opt-in and best-effort: fragments outside
/// CHARON's supported subset come back as `OutOfScope` rather than errors.

use std::process::Command;

use rem_domain::{
    entities::{VerificationResult, VerificationVerdict},
    errors::DomainError,
    ports::verification::VerificationPort,
    value_objects::FilePath,
};

pub struct CharonAeneasAdapter {
    /// Absolute path to the `charon` binary.
    pub charon_bin: String,
    /// Absolute path to the `aeneas` binary.
    pub aeneas_bin: String,
}

impl CharonAeneasAdapter {
    pub fn new(charon_bin: impl Into<String>, aeneas_bin: impl Into<String>) -> Self {
        Self {
            charon_bin: charon_bin.into(),
            aeneas_bin: aeneas_bin.into(),
        }
    }
}

impl VerificationPort for CharonAeneasAdapter {
    fn verify_equivalence(
        &self,
        source_file: &FilePath,
        original_fn_name: &str,
        extracted_fn_name: &str,
        project_root: &str,
    ) -> Result<VerificationResult, DomainError> {
        // ── Step 1: CHARON translation ────────────────────────────────────
        let charon_out = Command::new(&self.charon_bin)
            .args([
                "--input", source_file.as_str(),
                "--dest-dir", "/tmp/rem3_charon",
            ])
            .current_dir(project_root)
            .output()
            .map_err(|e| DomainError::UnsupportedConstruct(
                format!("CHARON launch failed: {e}")
            ))?;

        if !charon_out.status.success() {
            let stderr = String::from_utf8_lossy(&charon_out.stderr).into_owned();
            // Distinguish "unsupported" vs hard failure.
            let verdict = if stderr.contains("unsupported") || stderr.contains("dyn") {
                VerificationVerdict::OutOfScope {
                    reason: format!("CHARON: {}", stderr.lines().next().unwrap_or("unsupported")),
                }
            } else {
                VerificationVerdict::PipelineFailed { detail: stderr }
            };
            return Ok(VerificationResult { target: make_dummy_target(source_file), verdict });
        }

        // ── Step 2: AENEAS proof generation ──────────────────────────────
        let aeneas_out = Command::new(&self.aeneas_bin)
            .args([
                "/tmp/rem3_charon/output.llbc",
                "--backend", "coq",
                "--dest-dir", "/tmp/rem3_aeneas",
                "--original-fn", original_fn_name,
                "--extracted-fn", extracted_fn_name,
            ])
            .output()
            .map_err(|e| DomainError::UnsupportedConstruct(
                format!("AENEAS launch failed: {e}")
            ))?;

        if !aeneas_out.status.success() {
            let detail = String::from_utf8_lossy(&aeneas_out.stderr).into_owned();
            return Ok(VerificationResult {
                target: make_dummy_target(source_file),
                verdict: VerificationVerdict::PipelineFailed { detail },
            });
        }

        // ── Step 3: Coq proof check ───────────────────────────────────────
        let coqc_out = Command::new("coqc")
            .arg("/tmp/rem3_aeneas/Equivalence.v")
            .output()
            .map_err(|e| DomainError::UnsupportedConstruct(
                format!("coqc launch failed: {e}")
            ))?;

        let verdict = if coqc_out.status.success() {
            VerificationVerdict::Proved
        } else {
            VerificationVerdict::PipelineFailed {
                detail: String::from_utf8_lossy(&coqc_out.stderr).into_owned(),
            }
        };

        Ok(VerificationResult { target: make_dummy_target(source_file), verdict })
    }
}

/// Placeholder target for the result entity when we don't have full target info.
fn make_dummy_target(file: &FilePath) -> rem_domain::entities::ExtractionTarget {
    use rem_domain::value_objects::{ByteRange, FunctionName};
    // Safe to unwrap: all values are hard-coded valid.
    rem_domain::entities::ExtractionTarget::new(
        file.clone(),
        ByteRange::new(0, 1).unwrap(),
        FunctionName::new("extracted").unwrap(),
    )
    .unwrap()
}
