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
    /// `body`            — raw source of the selected fragment.
    /// `analysis`        — semantic analysis of the selection.
    /// `free_vars`       — ownership-refined free variables.
    /// `cf_reification`  — optional control-flow plan.
    pub fn generate(
        fn_name: &FunctionName,
        body: &str,
        analysis: &SelectionAnalysis,
        free_vars: &[FreeVariable],
        cf_reification: Option<&ControlFlowReification>,
    ) -> GeneratedExtraction {
        // Apply deref rewriting to the body for variables passed by reference
        let rewritten_body = rewrite_body_with_derefs(body, free_vars);
        
        let params = build_param_list(free_vars);
        let args   = build_arg_list(free_vars);
        let ret    = build_return_type(analysis);
        let asynck = if analysis.is_async { "async " } else { "" };
        let constk = if analysis.is_const { "const " } else { "" };
        let generics  = build_generic_params(analysis, free_vars);

        let ret_clause = if ret.is_empty() {
            String::new()
        } else {
            format!(" -> {ret}")
        };

        let extracted_fn_source = format!(
            "{constk}{asynck}fn {fn_name}{generics}({params}){ret_clause} {{\n{rewritten_body}\n}}"
        );

        let await_suffix = if analysis.is_async { ".await" } else { "" };
        let call_expr = format!("{fn_name}{generics}({args}){await_suffix}");

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

// ── internal body rewriter ──────────────────────────────────────────────────

fn rewrite_body_with_derefs(body: &str, vars: &[FreeVariable]) -> String {
    use ra_ap_syntax::{ast, AstNode, SyntaxElement};
    
    let refs_to_deref: std::collections::HashSet<String> = vars.iter()
        .filter(|v| matches!(v.ownership, OwnershipKind::SharedRef | OwnershipKind::MutRef))
        .map(|v| v.name.clone())
        .collect();
        
    if refs_to_deref.is_empty() {
        return body.to_string();
    }

    // Parse the body. Since it might not be a full source file, 
    // we wrap it in a dummy function to get a valid parse tree.
    let dummy_source = format!("fn _dummy() {{\n{}\n}}", body);
    let parse = ra_ap_syntax::SourceFile::parse(&dummy_source, ra_ap_syntax::Edition::Edition2021);
    let root = parse.tree();
    
    // Find the body of our dummy function
    let dummy_fn = root.syntax().descendants().find_map(ast::Fn::cast).expect("dummy fn not found");
    let body_node = dummy_fn.body().expect("dummy body not found");
    let body_range = body_node.syntax().text_range();
    
    // We want to replace all NameRef children that are in refs_to_deref.
    // We'll do it by building the string from pieces.
    let mut result = String::new();
    let mut last_pos = body_range.start() + ra_ap_syntax::TextSize::from(1); // skip '{'
    let end_pos = body_range.end() - ra_ap_syntax::TextSize::from(1); // skip '}'

    for node in body_node.syntax().descendants() {
        if let Some(name_ref) = ast::NameRef::cast(node.clone()) {
            let name_str = name_ref.to_string();
            if refs_to_deref.contains(&name_str) {
                // Check if it's already a child of a PrefixExpr with '*'
                let is_already_deref = name_ref.syntax().parent()
                    .and_then(ast::Path::cast)
                    .and_then(|p| p.syntax().parent())
                    .and_then(ast::PrefixExpr::cast)
                    .map(|pe| pe.op_kind() == Some(ast::UnaryOp::Deref))
                    .unwrap_or(false);
                
                if !is_already_deref {
                    let range = name_ref.syntax().text_range();
                    // Append text before this node
                    result.push_str(&dummy_source[last_pos.into()..range.start().into()]);
                    // Prepend '*'
                    result.push('*');
                    // Append the node itself
                    result.push_str(&dummy_source[range.start().into()..range.end().into()]);
                    last_pos = range.end();
                }
            }
        }
    }
    
    // Append remaining text
    result.push_str(&dummy_source[last_pos.into()..end_pos.into()]);
    
    // Trim leading/trailing whitespace that might have been added by our dummy wrap
    result.trim_matches(|c: char| c == '\n' || c == ' ').to_string()
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
    let params: Vec<String> = analysis.referenced_generics.clone();
    // Add lifetime parameters that appear in the free variable types.
    let lifetimes: Vec<String> = vars
        .iter()
        .flat_map(|v| extract_lifetimes(&v.ty))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let mut all = lifetimes;
    all.extend(params);
    if all.is_empty() {
        String::new()
    } else {
        format!("<{}>", all.join(", "))
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
