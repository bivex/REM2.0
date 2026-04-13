use crate::{
    errors::ExtractionFailure,
    ports::analysis::SelectionAnalysis,
    value_objects::ControlFlowKind,
};

/// Names for the variants of the auxiliary enum used to encode non-local
/// control flow in the extracted function.
pub const CF_RETURN_VARIANT:   &str = "Return";
pub const CF_BREAK_VARIANT:    &str = "Break";
pub const CF_CONTINUE_VARIANT: &str = "Continue";
pub const CF_TRY_VARIANT:      &str = "Err";   // for `?` propagation

/// Pure service: determines whether a non-local control-flow encoding is
/// needed and, if so, produces the Rust source text for the auxiliary enum
/// and the match arm that reconstructs the original behaviour at the call site.
pub struct ControlFlowAnalyzer;

/// Complete description of the reification needed for one extraction.
#[derive(Debug, Clone)]
pub struct ControlFlowReification {
    /// The name of the auxiliary enum to generate, e.g. `ExtractedCf0`.
    pub enum_name: String,
    /// Source text of the enum definition.
    pub enum_definition: String,
    /// Source text of the match expression the caller should use.
    pub caller_match: String,
}

impl ControlFlowAnalyzer {
    /// If the selection contains non-local control flow, produce the
    /// reification plan.  Returns `None` when no reification is needed.
    pub fn plan(
        analysis: &SelectionAnalysis,
        extracted_fn_name: &str,
        serial: u32,
    ) -> Result<Option<ControlFlowReification>, ExtractionFailure> {
        let exits = &analysis.control_flow_exits;
        if exits.is_empty() {
            return Ok(None);
        }

        let enum_name = format!("{}Cf{}", pascal(extracted_fn_name), serial);
        let mut variants = vec!["    Normal(T)".to_string()];
        let mut has_return = false;
        let mut has_try = false;

        for kind in exits {
            match kind {
                ControlFlowKind::Return   => { variants.push(format!("    {}(R)", CF_RETURN_VARIANT)); has_return = true; }
                ControlFlowKind::Break    => variants.push(format!("    {}", CF_BREAK_VARIANT)),
                ControlFlowKind::Continue => variants.push(format!("    {}", CF_CONTINUE_VARIANT)),
                ControlFlowKind::Try      => { variants.push(format!("    {}(E)", CF_TRY_VARIANT)); has_try = true; }
            }
        }

        // Build generic param list: only include params that are used
        let mut generic_params = vec!["T".to_string()];
        if has_return { generic_params.push("R".to_string()); }
        if has_try { generic_params.push("E".to_string()); }
        let generics = generic_params.join(", ");

        let enum_definition = format!(
            "#[allow(dead_code)]\nenum {enum_name}<{generics}> {{\n{}\n}}",
            variants.join(",\n")
        );

        // Build a match arm skeleton the caller can use.
        let mut arms = vec![format!("    {enum_name}::Normal(v) => v,")];
        for kind in exits {
            match kind {
                ControlFlowKind::Return   => arms.push(format!("    {enum_name}::Return(r) => return r,")),
                ControlFlowKind::Break    => arms.push(format!("    {enum_name}::Break => break,")),
                ControlFlowKind::Continue => arms.push(format!("    {enum_name}::Continue => continue,")),
                ControlFlowKind::Try      => arms.push(format!("    {enum_name}::Err(e) => return Err(e),")),
            }
        }
        let caller_match = format!("match result {{\n{}\n}}", arms.join("\n"));

        Ok(Some(ControlFlowReification { enum_name, enum_definition, caller_match }))
    }
}

/// Convert `snake_case` → `PascalCase` for enum naming.
fn pascal(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::analysis::SelectionAnalysis;

    fn empty_analysis(exits: Vec<ControlFlowKind>) -> SelectionAnalysis {
        SelectionAnalysis {
            free_variables: vec![],
            output_variables: vec![],
            control_flow_exits: exits,
            is_async: false,
            is_const: false,
            referenced_generics: vec![],
            enclosing_fn_return_type: None,
        }
    }

    #[test]
    fn no_exits_returns_none() {
        let result = ControlFlowAnalyzer::plan(&empty_analysis(vec![]), "foo", 0).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn return_exit_produces_enum() {
        let result = ControlFlowAnalyzer::plan(
            &empty_analysis(vec![ControlFlowKind::Return]),
            "compute_value",
            0,
        )
        .unwrap()
        .unwrap();
        assert!(result.enum_definition.contains("Return(R)"));
        assert!(result.caller_match.contains("return r"));
        assert_eq!(result.enum_name, "ComputeValueCf0");
    }

    #[test]
    fn pascal_conversion() {
        assert_eq!(pascal("compute_value"), "ComputeValue");
        assert_eq!(pascal("foo"), "Foo");
    }
}
