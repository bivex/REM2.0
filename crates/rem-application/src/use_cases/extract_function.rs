use tracing::{debug, info};

use rem_domain::{
    entities::ExtractionTarget,
    errors::ExtractionFailure,
    events::ExtractionEvent,
    ports::{
        analysis::CodeAnalysisPort,
        event_publisher::ExtractionEventPublisher,
        filesystem::FileSystemPort,
        repair::LifetimeRepairPort,
        syntax_rewrite::SyntaxRewritePort,
    },
    services::{
        code_generator::CodeGenerator,
        control_flow_analyzer::ControlFlowAnalyzer,
        lifetime_repairer::LifetimeRepairer,
        ownership_oracle::OwnershipOracle,
    },
    value_objects::{ByteRange, FilePath, FunctionName, OwnershipKind},
};

use crate::{
    dto::{ExtractFunctionRequest, ExtractFunctionResponse, ExtractionStats},
    errors::ApplicationError,
};

/// **Use-case: Extract Function**
///
/// Orchestrates the full pipeline:
///   analyse → ownership → control-flow → code-gen → write → repair loop
///
/// All I/O dependencies are injected.  The use-case itself contains no
/// direct infrastructure calls.
pub struct ExtractFunctionUseCase {
    pub analysis:        Box<dyn CodeAnalysisPort>,
    pub repair:          Box<dyn LifetimeRepairPort>,
    pub fs:              Box<dyn FileSystemPort>,
    pub publisher:       Box<dyn ExtractionEventPublisher>,
    pub syntax_rewriter: Box<dyn SyntaxRewritePort>,
}

impl ExtractFunctionUseCase {
    pub fn execute(
        &self,
        req: ExtractFunctionRequest,
    ) -> Result<ExtractFunctionResponse, ApplicationError> {
        // ── 1. Validate & construct domain value objects ──────────────────
        let file = FilePath::new(&req.file)?;
        let range = ByteRange::new(req.start_byte, req.end_byte)?;
        let fn_name = FunctionName::new(&req.extracted_fn_name)
            .map_err(|e| ApplicationError::InvalidInput(e.to_string()))?;

        let target = ExtractionTarget::new(file.clone(), range, fn_name.clone())
            .map_err(ApplicationError::Domain)?;

        self.publisher.publish(ExtractionEvent::Started { target: target.clone() });
        info!(file = %file, start = range.start, end = range.end, name = %fn_name, "extraction started");

        // ── 2. Locate project root ────────────────────────────────────────
        let project_root = match &req.project_root {
            Some(r) => r.clone(),
            None => self.fs.find_cargo_toml(req.file.as_str())
                .map_err(ApplicationError::Domain)?,
        };

        // ── 3. Load workspace and analyse the selection ───────────────────
        self.analysis.load_workspace(&project_root)?;

        let analysis = match self.analysis.analyse_selection(&file, range) {
            Ok(a) => a,
            Err(e) => {
                let reason = ExtractionFailure::AnalysisFailed(e.to_string());
                self.publisher.publish(ExtractionEvent::ExtractionFailed {
                    target: target.clone(),
                    reason: reason.clone(),
                });
                return Ok(failure_response(reason));
            }
        };

        self.publisher.publish(ExtractionEvent::AnalysisCompleted {
            target: target.clone(),
            free_variables_count: analysis.free_variables.len(),
        });
        debug!(?analysis.control_flow_exits, "analysis complete");

        // ── 4. Refine ownership for each free variable ────────────────────
        let free_vars = OwnershipOracle::refine(&analysis);

        // ── 5. Plan control-flow reification ─────────────────────────────
        let cf_plan = match ControlFlowAnalyzer::plan(&analysis, fn_name.as_str(), 0) {
            Ok(p) => p,
            Err(e) => {
                self.publisher.publish(ExtractionEvent::ExtractionFailed {
                    target: target.clone(),
                    reason: e.clone(),
                });
                return Ok(failure_response(e));
            }
        };

        if !analysis.control_flow_exits.is_empty() {
            self.publisher.publish(ExtractionEvent::ControlFlowReificationRequired {
                target: target.clone(),
                kinds: analysis.control_flow_exits.clone(),
            });
        }

        // ── 6. Read the source file ───────────────────────────────────────
        let source = self.fs.read_to_string(file.as_str())
            .map_err(ApplicationError::Domain)?;

        // ── 7. Carve out the selected fragment ────────────────────────────
        let body = source
            .get(range.start as usize..range.end as usize)
            .unwrap_or("")
            .to_string();

        // ── 7b. Rewrite body with deref operators for ref-passed variables ──
        let ref_var_names: Vec<String> = free_vars.iter()
            .filter(|v| matches!(v.ownership, OwnershipKind::SharedRef | OwnershipKind::MutRef))
            .map(|v| v.name.clone())
            .collect();
        let rewritten_body = self.syntax_rewriter.rewrite_body_with_derefs(&body, &ref_var_names);

        // ── 7c. Rewrite control-flow exits in the body ──────────────────────
        let rewritten_body = match &cf_plan {
            Some(cf) => rewrite_cf_body(&rewritten_body, &cf.enum_name, &analysis),
            None => rewritten_body,
        };

        // ── 8. Generate initial extraction text ───────────────────────────
        let generated = CodeGenerator::generate(
            &fn_name,
            &rewritten_body,
            &analysis,
            &free_vars,
            cf_plan.as_ref(),
        );

        // ── 9. Build the patched source ───────────────────────────────────
        let mut patched = source.clone();
        patched.replace_range(
            range.start as usize..range.end as usize,
            &generated.call_site_replacement,
        );
        // Append CF enum + extracted fn after the enclosing function.
        if let Some(cf_src) = &generated.cf_enum_source {
            patched.push('\n');
            patched.push_str(cf_src);
        }
        patched.push('\n');
        patched.push_str(&generated.extracted_fn_source);

        // ── 10. Lifetime repair loop ──────────────────────────────────────
        let mut repair_iterations: u32 = 0;
        let publisher_ref = &*self.publisher;
        let target_ref = &target;

        let final_source = match LifetimeRepairer::repair(
            &project_root,
            &file,
            patched,
            &*self.repair,
            |iter, code| {
                repair_iterations = iter;
                publisher_ref.publish(ExtractionEvent::LifetimeRepairIteration {
                    target: target_ref.clone(),
                    iteration: iter,
                    error_code: code.to_string(),
                });
            },
        ) {
            Ok(src) => src,
            Err(e) => {
                self.publisher.publish(ExtractionEvent::ExtractionFailed {
                    target: target.clone(),
                    reason: e.clone(),
                });
                return Ok(failure_response(e));
            }
        };

        if repair_iterations > 0 {
            self.publisher.publish(ExtractionEvent::LifetimeRepairSucceeded {
                target: target.clone(),
                total_iterations: repair_iterations,
            });
        }

        // ── 11. Write the result back ─────────────────────────────────────
        self.fs.write_string(file.as_str(), &final_source)
            .map_err(ApplicationError::Domain)?;

        self.publisher.publish(ExtractionEvent::ExtractionSucceeded { target: target.clone() });
        info!(name = %fn_name, iterations = repair_iterations, "extraction succeeded");

        let summary = format!(
            "Extracted `{}` ({} vars, {} CF exits, {} repair iteration(s))",
            fn_name,
            free_vars.len(),
            analysis.control_flow_exits.len(),
            repair_iterations,
        );

        Ok(ExtractFunctionResponse {
            success: true,
            new_file_content: Some(final_source),
            summary,
            error: None,
            stats: ExtractionStats {
                lifetime_repair_iterations: repair_iterations,
                control_flow_reified: cf_plan.is_some(),
                is_async: analysis.is_async,
                is_const: analysis.is_const,
            },
            verification: None, // filled by CLI if req.verify
        })
    }
}

fn failure_response(reason: ExtractionFailure) -> ExtractFunctionResponse {
    ExtractFunctionResponse {
        success: false,
        new_file_content: None,
        summary: format!("extraction failed: {reason}"),
        error: Some(reason.to_string()),
        stats: ExtractionStats::default(),
        verification: None,
    }
}

/// Strip `// ...` line comments from source text, preserving newlines.
fn strip_line_comments(src: &str) -> String {
    let mut result = String::with_capacity(src.len());
    let mut in_string = false;
    let mut string_delim = b'"';
    let bytes = src.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if in_string {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                result.push(bytes[i] as char);
                result.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if bytes[i] == string_delim {
                in_string = false;
            }
            result.push(bytes[i] as char);
            i += 1;
        } else if bytes[i] == b'"' || bytes[i] == b'\'' {
            in_string = true;
            string_delim = bytes[i];
            result.push(bytes[i] as char);
            i += 1;
        } else if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // Skip until end of line
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

/// Rewrite control-flow exits in the extracted body to return CF enum variants.
///
/// - `break`       → `return {enum_name}::Break`
/// - `continue`    → `return {enum_name}::Continue`
/// - `return expr` → `return {enum_name}::Return(expr)`
///
/// Then wraps the final value in `{enum_name}::Normal(...)`.
fn rewrite_cf_body(
    body: &str,
    enum_name: &str,
    analysis: &rem_domain::ports::analysis::SelectionAnalysis,
) -> String {
    use rem_domain::value_objects::ControlFlowKind;

    let has_break = analysis.control_flow_exits.contains(&ControlFlowKind::Break);
    let has_continue = analysis.control_flow_exits.contains(&ControlFlowKind::Continue);
    let has_return = analysis.control_flow_exits.contains(&ControlFlowKind::Return);

    // Strip line comments to avoid false matches on keywords inside comments
    let cleaned = strip_line_comments(body);

    let mut result = String::with_capacity(cleaned.len() + 512);
    let bytes = cleaned.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        // Check for `return ` — handle first to avoid double-rewriting
        if has_return && bytes[pos..].starts_with(b"return ") {
            let after_ret = pos + 7;
            // Find the semicolon/comma that ends this return statement
            let mut depth = 0i32;
            let mut end = after_ret;
            while end < bytes.len() {
                match bytes[end] {
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => depth -= 1,
                    b';' | b',' if depth == 0 => break,
                    _ => {}
                }
                end += 1;
            }
            let expr = cleaned[after_ret..end].trim();
            let terminator = &cleaned[end..end + 1]; // ; or ,
            result.push_str(&format!("return {}::Return({}){}", enum_name, expr, terminator));
            pos = end + 1;
        }
        // Check for `break` followed by ; or , or }
        else if has_break && bytes[pos..].starts_with(b"break") {
            let after_break = pos + 5;
            // skip whitespace
            let mut next = after_break;
            while next < bytes.len() && bytes[next] == b' ' { next += 1; }
            if next < bytes.len() && (bytes[next] == b';' || bytes[next] == b',' || bytes[next] == b'}') {
                let terminator = &cleaned[next..next + 1];
                result.push_str(&format!("return {}::Break{}", enum_name, terminator));
                pos = next + 1;
            } else {
                result.push(bytes[pos] as char);
                pos += 1;
            }
        }
        // Check for `continue` followed by ; or , or }
        else if has_continue && bytes[pos..].starts_with(b"continue") {
            let after_cont = pos + 8;
            let mut next = after_cont;
            while next < bytes.len() && bytes[next] == b' ' { next += 1; }
            if next < bytes.len() && (bytes[next] == b';' || bytes[next] == b',' || bytes[next] == b'}') {
                let terminator = &cleaned[next..next + 1];
                result.push_str(&format!("return {}::Continue{}", enum_name, terminator));
                pos = next + 1;
            } else {
                result.push(bytes[pos] as char);
                pos += 1;
            }
        }
        else {
            result.push(bytes[pos] as char);
            pos += 1;
        }
    }

    // Wrap the tail: append `Enum::Normal(())` or `Enum::Normal(<output>)` at the end
    let tail = if analysis.output_variables.is_empty() {
        format!("{}::Normal(())", enum_name)
    } else {
        let out_names: Vec<&str> = analysis.output_variables.iter().map(|v| v.name.as_str()).collect();
        format!("{}::Normal({})", enum_name, out_names.join(", "))
    };

    format!("{}\n{}", result, tail)
}

