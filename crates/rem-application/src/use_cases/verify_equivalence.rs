use tracing::info;

use rem_domain::{
    entities::{VerificationVerdict},
    ports::{filesystem::FileSystemPort, verification::VerificationPort},
    value_objects::FilePath,
};

use crate::{
    dto::{VerificationResponse, VerifyEquivalenceRequest},
    errors::ApplicationError,
};

/// **Use-case: Verify Equivalence**
///
/// Delegates to the CHARON / AENEAS pipeline through `VerificationPort`.
/// Returns a structured `VerificationResponse` describing the verdict.
pub struct VerifyEquivalenceUseCase {
    pub verification: Box<dyn VerificationPort>,
    pub fs:           Box<dyn FileSystemPort>,
}

impl VerifyEquivalenceUseCase {
    pub fn execute(
        &self,
        req: VerifyEquivalenceRequest,
    ) -> Result<VerificationResponse, ApplicationError> {
        let file = FilePath::new(&req.file)?;

        info!(
            file = %file,
            original  = %req.original_fn_name,
            extracted = %req.extracted_fn_name,
            "verification started"
        );

        let result = self
            .verification
            .verify_equivalence(
                &file,
                &req.original_fn_name,
                &req.extracted_fn_name,
                &req.project_root,
            )
            .map_err(ApplicationError::Domain)?;

        let response = match result.verdict {
            VerificationVerdict::Proved => VerificationResponse {
                verdict: "proved".into(),
                detail: None,
            },
            VerificationVerdict::CounterexampleFound { detail } => VerificationResponse {
                verdict: "counterexample".into(),
                detail: Some(detail),
            },
            VerificationVerdict::OutOfScope { reason } => VerificationResponse {
                verdict: "out_of_scope".into(),
                detail: Some(reason),
            },
            VerificationVerdict::PipelineFailed { detail } => VerificationResponse {
                verdict: "failed".into(),
                detail: Some(detail),
            },
        };

        info!(verdict = %response.verdict, "verification complete");
        Ok(response)
    }
}
