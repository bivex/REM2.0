use anyhow::{Context, Result};
use tracing::error;

use rem_application::{
    dto::{ExtractFunctionRequest, VerifyEquivalenceRequest},
    use_cases::{
        extract_function::ExtractFunctionUseCase,
        verify_equivalence::VerifyEquivalenceUseCase,
    },
};
use rem_infrastructure::adapters::{
    cargo::CargoCheckAdapter,
    charon_aeneas::CharonAeneasAdapter,
    event_publisher::TracingEventPublisher,
    filesystem::OsFileSystemAdapter,
    rust_analyzer::RustAnalyzerAdapter,
};

use crate::cli::{ExtractArgs, VerifyArgs};

// ── extract handler ──────────────────────────────────────────────────────────

pub fn handle_extract(args: ExtractArgs) -> Result<()> {
    let use_case = ExtractFunctionUseCase {
        analysis:  Box::new(RustAnalyzerAdapter::new()),
        repair:    Box::new(CargoCheckAdapter),
        fs:        Box::new(OsFileSystemAdapter),
        publisher: Box::new(TracingEventPublisher),
    };

    let request = ExtractFunctionRequest {
        file:              args.file,
        start_byte:        args.start,
        end_byte:          args.end,
        extracted_fn_name: args.name,
        project_root:      args.project_root,
        verify:            args.verify,
    };

    // Capture fields we'll need after `execute` consumes `request`.
    let extracted_fn_name = request.extracted_fn_name.clone();
    let project_root_clone = request.project_root.clone().unwrap_or_default();

    let response = use_case.execute(request)
        .context("extract-function use-case")?;

    // Optional verification pass.
    let mut response = response;
    if args.verify && response.success {
        let verify_uc = VerifyEquivalenceUseCase {
            verification: Box::new(CharonAeneasAdapter::new(args.charon_bin, args.aeneas_bin)),
            fs:           Box::new(OsFileSystemAdapter),
        };
        let vreq = VerifyEquivalenceRequest {
            file:              response.new_file_content.as_deref().unwrap_or("").to_string(),
            original_fn_name:  "__original__".into(),
            extracted_fn_name,
            project_root:      project_root_clone,
        };
        match verify_uc.execute(vreq) {
            Ok(vr) => response.verification = Some(vr),
            Err(e) => error!("verification failed: {e}"),
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_extract_response(&response);
    }

    if response.success { Ok(()) } else { anyhow::bail!(response.error.unwrap_or_default()) }
}

// ── verify handler ───────────────────────────────────────────────────────────

pub fn handle_verify(args: VerifyArgs) -> Result<()> {
    let use_case = VerifyEquivalenceUseCase {
        verification: Box::new(CharonAeneasAdapter::new(args.charon_bin, args.aeneas_bin)),
        fs:           Box::new(OsFileSystemAdapter),
    };

    let request = VerifyEquivalenceRequest {
        file:              args.file,
        original_fn_name:  args.original,
        extracted_fn_name: args.extracted,
        project_root:      args.project_root,
    };

    let response = use_case.execute(request)
        .context("verify-equivalence use-case")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        print_verify_response(&response);
    }

    match response.verdict.as_str() {
        "proved" => Ok(()),
        other    => anyhow::bail!("verification verdict: {other}"),
    }
}

// ── formatters ───────────────────────────────────────────────────────────────

fn print_extract_response(r: &rem_application::dto::ExtractFunctionResponse) {
    if r.success {
        println!("✓ {}", r.summary);
        let s = &r.stats;
        println!(
            "  repairs={} | cf_reified={} | async={} | const={}",
            s.lifetime_repair_iterations, s.control_flow_reified, s.is_async, s.is_const
        );
        if let Some(v) = &r.verification {
            println!("  verification: {}", v.verdict);
            if let Some(d) = &v.detail { println!("    {d}"); }
        }
    } else {
        eprintln!("✗ {}", r.summary);
    }
}

fn print_verify_response(r: &rem_application::dto::VerificationResponse) {
    println!("verdict: {}", r.verdict);
    if let Some(d) = &r.detail { println!("  {d}"); }
}
