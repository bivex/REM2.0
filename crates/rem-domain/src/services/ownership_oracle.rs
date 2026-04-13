use crate::{
    ports::analysis::{FreeVariable, SelectionAnalysis},
    value_objects::OwnershipKind,
};

/// Decides the passing convention for each free variable that crosses the
/// extracted-function boundary.
///
/// Rules (in priority order):
///  1. If the variable is mutated inside the selection AND used after it
///     → `MutRef`
///  2. If the variable is mutated inside the selection but NOT used after
///     → `Owned` (the callee can take ownership)
///  3. If the variable is only read inside the selection
///     → `SharedRef`
///
/// This is a pure function: given the same analysis it always produces the
/// same assignments.
pub struct OwnershipOracle;

impl OwnershipOracle {
    /// Refine the ownership kind for each free variable using control-flow
    /// information (whether the variable is live after the extraction point).
    ///
    /// Note: `analysis.free_variables` already carry a first-pass `ownership`
    /// computed by the analysis adapter; this service applies policy on top.
    pub fn refine(analysis: &SelectionAnalysis) -> Vec<FreeVariable> {
        analysis
            .free_variables
            .iter()
            .map(|v| {
                let ownership = match v.ownership {
                    // Adapter already determined it is mutated. Check if
                    // it also appears in the output set (live after extraction).
                    OwnershipKind::MutRef => OwnershipKind::MutRef,
                    OwnershipKind::Owned => {
                        let used_after = analysis
                            .output_variables
                            .iter()
                            .any(|out| out.name == v.name);
                        if used_after {
                            OwnershipKind::MutRef
                        } else {
                            OwnershipKind::Owned
                        }
                    }
                    // Read-only — always a shared reference.
                    OwnershipKind::SharedRef => OwnershipKind::SharedRef,
                };
                FreeVariable {
                    name: v.name.clone(),
                    ty: v.ty.clone(),
                    ownership,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ports::analysis::OutputVariable, value_objects::OwnershipKind};

    fn make_var(name: &str, ownership: OwnershipKind) -> FreeVariable {
        FreeVariable { name: name.into(), ty: "i32".into(), ownership }
    }

    #[test]
    fn mutated_and_used_after_becomes_mut_ref() {
        let analysis = SelectionAnalysis {
            free_variables: vec![make_var("x", OwnershipKind::Owned)],
            output_variables: vec![OutputVariable { name: "x".into(), ty: "i32".into() }],
            control_flow_exits: vec![],
            is_async: false,
            is_const: false,
            referenced_generics: vec![],
        };
        let refined = OwnershipOracle::refine(&analysis);
        assert_eq!(refined[0].ownership, OwnershipKind::MutRef);
    }

    #[test]
    fn read_only_stays_shared_ref() {
        let analysis = SelectionAnalysis {
            free_variables: vec![make_var("y", OwnershipKind::SharedRef)],
            output_variables: vec![],
            control_flow_exits: vec![],
            is_async: false,
            is_const: false,
            referenced_generics: vec![],
        };
        let refined = OwnershipOracle::refine(&analysis);
        assert_eq!(refined[0].ownership, OwnershipKind::SharedRef);
    }
}
