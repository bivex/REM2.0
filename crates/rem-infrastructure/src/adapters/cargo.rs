/// Adapter: cargo check feedback → `LifetimeRepairPort`
///
/// Invokes `cargo check --message-format=json` as a subprocess and parses
/// the structured compiler diagnostics into the domain's `RepairOutcome`.

use std::process::Command;

use rem_domain::{
    errors::DomainError,
    ports::repair::{CompilerDiagnostic, LifetimeRepairPort, RepairOutcome},
};

use serde::Deserialize;

pub struct CargoCheckAdapter;

impl LifetimeRepairPort for CargoCheckAdapter {
    /// Writes `source_patch` to a temporary location, runs `cargo check`,
    /// and returns diagnostics.
    ///
    /// NOTE: In the full implementation, the patch is applied to the VFS
    /// in-memory and a lightweight check is performed.  This initial version
    /// runs `cargo check` against the real on-disk file to keep the adapter
    /// simple and correct.
    fn check(
        &self,
        project_root: &str,
        file: &rem_domain::value_objects::FilePath,
        source_patch: &str,
    ) -> Result<RepairOutcome, DomainError> {
        let original_content = std::fs::read_to_string(file.as_str())
            .map_err(|e| DomainError::InvalidFilePath(format!("backup failed: {e}")))?;

        std::fs::write(file.as_str(), source_patch)
            .map_err(|e| DomainError::InvalidFilePath(format!("write failed: {e}")))?;

        let output = Command::new("cargo")
            .args(["check", "--message-format=json", "--quiet"])
            .current_dir(project_root)
            .output();

        // Always restore the file after checking
        let restore_result = std::fs::write(file.as_str(), original_content);

        let output = output.map_err(|e| {
            DomainError::UnsupportedConstruct(format!("failed to spawn cargo check: {e}"))
        })?;

        if let Err(e) = restore_result {
            return Err(DomainError::InvalidFilePath(format!("restore failed: {e}")));
        }

        if output.status.success() {
            return Ok(RepairOutcome::Accepted);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let diagnostics = parse_cargo_diagnostics(&stdout);

        Ok(RepairOutcome::Rejected { diagnostics })
    }
}

// ── Cargo JSON message parsing ────────────────────────────────────────────────

#[derive(Deserialize, Debug)]
struct CargoMessage {
    reason: String,
    message: Option<MessageDetail>,
}

#[derive(Deserialize, Debug)]
struct MessageDetail {
    code: Option<CodeDetail>,
    message: String,
    spans:   Vec<SpanDetail>,
    children: Vec<ChildDetail>,
}

#[derive(Deserialize, Debug, Default)]
struct ChildDetail {
    message: String,
}

#[derive(Deserialize, Debug)]
struct CodeDetail {
    code: String,
}

#[derive(Deserialize, Debug)]
struct SpanDetail {
    text: Vec<SpanText>,
}

#[derive(Deserialize, Debug)]
struct SpanText {
    text: String,
}

fn parse_cargo_diagnostics(json_lines: &str) -> Vec<CompilerDiagnostic> {
    json_lines
        .lines()
        .filter_map(|line| serde_json::from_str::<CargoMessage>(line).ok())
        .filter(|m| m.reason == "compiler-message")
        .filter_map(|m| m.message)
        .filter(|m| m.code.is_some())
        .map(|m| {
            let help = m.children.iter()
                .map(|c| c.message.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            CompilerDiagnostic {
                error_code: m.code.unwrap().code,
                message: m.message,
                span_text: m.spans.first()
                    .and_then(|s| s.text.first())
                    .map(|t| t.text.clone()),
                help_text: if help.is_empty() { None } else { Some(help) },
            }
        })
        .collect()
}
