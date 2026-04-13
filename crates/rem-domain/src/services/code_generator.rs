use crate::{
    ports::analysis::{FreeVariable, OutputVariable, SelectionAnalysis},
    services::control_flow_analyzer::ControlFlowReification,
    value_objects::{FunctionName, OwnershipKind},
};

/// Pure code-generation service.
///
/// Produces the textual Rust source for:
///  - the extracted function definition
///  - the replacement call expression at the original call-site
///
/// No I/O, no state — given the same inputs it always returns the same text.
/// Body rewriting (deref insertion) is handled by `SyntaxRewritePort` before
/// calling `generate`.
pub struct CodeGenerator;

/// All generated text for one extraction.
#[derive(Debug, Clone)]
pub struct GeneratedExtraction {
    /// Full source text of the new extracted function (including signature).
    pub extracted_fn_source: String,
    /// Expression / statement that replaces the selected fragment in the caller.
    pub call_site_replacement: String,
    /// Auxiliary control-flow enum source, if reification was required.
    pub cf_enum_source: Option<String>,
}

impl CodeGenerator {
    /// Generate extraction source from semantic inputs.
    ///
    /// `fn_name`         — validated extracted function name.
    /// `rewritten_body`  — body with deref operators already inserted.
    /// `analysis`        — semantic analysis of the selection.
    /// `free_vars`       — ownership-refined free variables.
    /// `cf_reification`  — optional control-flow plan.
    pub fn generate(
        fn_name: &FunctionName,
        rewritten_body: &str,
        analysis: &SelectionAnalysis,
        free_vars: &[FreeVariable],
        cf_reification: Option<&ControlFlowReification>,
    ) -> GeneratedExtraction {
        let params = build_param_list(free_vars);
        let args   = build_arg_list(free_vars);
        let ret    = build_return_type(analysis);
        let asynck = if analysis.is_async { "async " } else { "" };
        let constk = if analysis.is_const { "const " } else { "" };
        let generics       = build_generic_params(analysis, free_vars);
        let call_generics  = build_call_site_generics(analysis);

        let ret_clause = if ret.is_empty() {
            String::new()
        } else {
            format!(" -> {ret}")
        };

        let extracted_fn_source = format!(
            "{constk}{asynck}fn {fn_name}{generics}({params}){ret_clause} {{\n{rewritten_body}\n}}"
        );

        let await_suffix = if analysis.is_async { ".await" } else { "" };
        let call_expr = format!("{fn_name}{call_generics}({args}){await_suffix}");

        let call_site_replacement = match cf_reification {
            None => {
                if analysis.output_variables.is_empty() {
                    format!("{call_expr};")
                } else {
                    let lhs = build_output_lhs(&analysis.output_variables);
                    format!("let {lhs} = {call_expr};")
                }
            }
            Some(cf) => {
                let lhs = if analysis.output_variables.is_empty() {
                    "_".to_string()
                } else {
                    build_output_lhs(&analysis.output_variables)
                };
                format!(
                    "let {lhs} = {{\n    let result = {call_expr};\n    {}\n}};",
                    cf.caller_match
                )
            }
        };

        GeneratedExtraction {
            extracted_fn_source,
            call_site_replacement,
            cf_enum_source: cf_reification.map(|cf| cf.enum_definition.clone()),
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn build_param_list(vars: &[FreeVariable]) -> String {
    vars.iter()
        .map(|v| match v.ownership {
            OwnershipKind::Owned     => format!("{}: {}", v.name, v.ty),
            OwnershipKind::SharedRef => format!("{}: &{}", v.name, v.ty),
            OwnershipKind::MutRef    => format!("{}: &mut {}", v.name, v.ty),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_arg_list(vars: &[FreeVariable]) -> String {
    vars.iter()
        .map(|v| match v.ownership {
            OwnershipKind::Owned     => v.name.clone(),
            OwnershipKind::SharedRef => format!("&{}", v.name),
            OwnershipKind::MutRef    => format!("&mut {}", v.name),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn build_return_type(analysis: &SelectionAnalysis) -> String {
    match analysis.output_variables.len() {
        0 => String::new(),
        1 => analysis.output_variables[0].ty.clone(),
        _ => {
            let tys: Vec<_> = analysis.output_variables.iter().map(|v| v.ty.as_str()).collect();
            format!("({})", tys.join(", "))
        }
    }
}

fn build_output_lhs(outputs: &[OutputVariable]) -> String {
    if outputs.len() == 1 {
        outputs[0].name.clone()
    } else {
        let names: Vec<_> = outputs.iter().map(|v| v.name.as_str()).collect();
        format!("({})", names.join(", "))
    }
}

fn build_generic_params(analysis: &SelectionAnalysis, vars: &[FreeVariable]) -> String {
    let params_strs: Vec<String> = analysis.referenced_generics.iter()
        .map(|g| g.full_definition.clone())
        .collect();

    // Add lifetime parameters that appear in the free variable types.
    let lifetimes: Vec<String> = vars
        .iter()
        .flat_map(|v| extract_lifetimes(&v.ty))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut all = lifetimes;
    all.extend(params_strs);

    if all.is_empty() {
        String::new()
    } else {
        format!("<{}>", all.join(", "))
    }
}

/// Build generic parameter list for the call site (names only, no bounds).
fn build_call_site_generics(analysis: &SelectionAnalysis) -> String {
    let names: Vec<&str> = analysis.referenced_generics.iter()
        .map(|g| g.name.as_str())
        .collect();

    if names.is_empty() {
        String::new()
    } else {
        format!("::{}", format!("<{}>", names.join(", ")))
    }
}

/// Very lightweight: pull `'name` tokens out of a type string.
fn extract_lifetimes(ty: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = ty.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\'' {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let lt = &ty[start..i];
            if lt.len() > 1 && lt != "'static" {
                out.push(lt.to_string());
            }
        } else {
            i += 1;
        }
    }
    out
}
