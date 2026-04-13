/// Adapter: rust-analyzer → `CodeAnalysisPort`
///
/// Uses the `ra-ap-*` public API crates to load a Cargo workspace into
/// memory and answer semantic questions about selected fragments without
/// spawning `rustc` as a subprocess.

use std::sync::Mutex;

use ra_ap_hir::{HirDisplay, PathResolution, Semantics};
use ra_ap_ide::{AnalysisHost, FileId, TextRange, TextSize};
use ra_ap_load_cargo::{load_workspace_at, LoadCargoConfig, ProcMacroServerChoice};
use ra_ap_project_model::CargoConfig;
use ra_ap_syntax::{ast, AstNode};
use ra_ap_vfs::{AbsPathBuf, Vfs};

use rem_domain::{
    errors::DomainError,
    ports::analysis::{CodeAnalysisPort, SelectionAnalysis},
    value_objects::{ByteRange, FilePath},
};

pub struct RustAnalyzerAdapter {
    inner: Mutex<Option<AdapterInner>>,
}

struct AdapterInner {
    host: AnalysisHost,
    vfs: Vfs,
}

impl RustAnalyzerAdapter {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

impl Default for RustAnalyzerAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeAnalysisPort for RustAnalyzerAdapter {
    fn load_workspace(&self, project_root: &str) -> Result<(), DomainError> {
        use std::path::Path;

        let root = Path::new(project_root);
        let cargo_cfg = CargoConfig::default();
        let load_cfg = LoadCargoConfig {
            load_out_dirs_from_check: false,
            with_proc_macro_server: ProcMacroServerChoice::None,
            prefill_caches: false,
            num_worker_threads: 0,
            proc_macro_processes: Default::default(),
        };

        let (db, vfs, _proc_macro) = load_workspace_at(root, &cargo_cfg, &load_cfg, &|_| {})
            .map_err(|e| {
            DomainError::InvalidFilePath(format!(
                "workspace load failed at `{project_root}`: {e}"
            ))
        })?;

        let host = AnalysisHost::with_database(db);
        *self.inner.lock().unwrap() = Some(AdapterInner { host, vfs });
        Ok(())
    }

    fn analyse_selection(
        &self,
        file: &FilePath,
        range: ByteRange,
    ) -> Result<SelectionAnalysis, DomainError> {
        let guard = self.inner.lock().unwrap();
        let inner = guard.as_ref().ok_or_else(|| {
            DomainError::InvalidFilePath(
                "workspace not loaded — call load_workspace first".into(),
            )
        })?;

        let file_id = resolve_file_id(&inner.vfs, file)?;

        let text_range = TextRange::new(
            TextSize::from(range.start),
            TextSize::from(range.end),
        );

        let db = inner.host.raw_database();

        ra_ap_hir::attach_db(db, || {
            let sema = Semantics::new(db);
            let editioned_file_id = sema.attach_first_edition(file_id);
            let source_file = sema.parse(editioned_file_id);
            let syntax = source_file.syntax();

            let krate = sema.first_crate(file_id).unwrap_or_else(|| {
                ra_ap_hir::Crate::all(db).into_iter().next().expect("no crates in database")
            });
            let display_target = krate.to_display_target(db);

            let mut free_variables = Vec::new();
            let mut output_variables = Vec::new();
            let mut control_flow_exits = Vec::new();
            let mut is_async = false;
            let mut is_const = false;
            let mut seen_free = std::collections::HashSet::new();
            let mut seen_generics = std::collections::HashSet::new();
            let mut referenced_generics = Vec::new();

            // ── Detect async/const context ──────────────────────────────────
            // Walk ancestors from the selection start to find the enclosing fn
            let pos = TextSize::from(range.start);
            if let Some(fn_def) = syntax
                .token_at_offset(pos)
                .next()
                .and_then(|t| t.parent_ancestors().find_map(ast::Fn::cast))
            {
                is_async = fn_def.async_token().is_some();
                is_const = fn_def.const_token().is_some();
            }

            // ── 1. Find free variables, output variables, and generics ──────
            for element in syntax
                .descendants_with_tokens()
                .filter(|el| text_range.contains_range(el.text_range()))
            {
                let Some(token) = element.into_token() else { continue };
                if token.kind() != ra_ap_syntax::SyntaxKind::IDENT {
                    continue;
                }

                for expanded in sema.descend_into_macros(token.clone()) {
                    let mut curr = Some(expanded.parent().expect("token must have a parent"));
                    while let Some(node) = curr.take() {
                        if let Some(path) = ast::Path::cast(node.clone()) {
                            if let Some(res) = sema.resolve_path(&path) {
                                match res {
                                    PathResolution::Local(local) => {
                                        let sources = local.sources(db);
                                        let Some(source) = sources.first() else { continue };
                                        let def_range = source.source.value.syntax().text_range();
                                        let name = path.to_string();
                                        let defined_inside = text_range.contains_range(def_range);

                                        if !defined_inside {
                                            // Free variable: defined outside, used inside
                                            if seen_free.insert(name.clone()) {
                                                // Ownership is determined later by refine_ownership.
                                                // Use a placeholder — it will be overwritten.
                                                let ownership =
                                                    rem_domain::value_objects::OwnershipKind::SharedRef;

                                                let ty = local.ty(db);
                                                let mut ty_str_opt = None;

                                                // Check if it's a generic type parameter
                                                if let Some(tp) = ty.as_type_param(db) {
                                                    let tp_name = tp.name(db).as_str().to_string();
                                                    ty_str_opt = Some(tp_name.clone());
                                                    if seen_generics.insert(tp_name.clone()) {
                                                        let bounds = tp.trait_bounds(db);
                                                        let full_definition = if !bounds.is_empty() {
                                                            let bounds_strs: Vec<String> = bounds
                                                                .iter()
                                                                .map(|b| {
                                                                    b.name(db).as_str().to_string()
                                                                })
                                                                .collect();
                                                            format!(
                                                                "{}: {}",
                                                                tp_name,
                                                                bounds_strs.join(" + ")
                                                            )
                                                        } else {
                                                            tp_name.clone()
                                                        };
                                                        referenced_generics
                                                            .push(
                                                                rem_domain::ports::analysis::GenericParam {
                                                                    name: tp_name,
                                                                    full_definition,
                                                                },
                                                            );
                                                    }
                                                }

                                                let ty_str = ty_str_opt.unwrap_or_else(|| {
                                                    ty.display(db, display_target).to_string()
                                                });

                                                free_variables
                                                    .push(rem_domain::ports::analysis::FreeVariable {
                                                        name,
                                                        ty: ty_str,
                                                        ownership,
                                                    });
                                            }
                                        } else {
                                            // Output variable: defined inside, used after selection
                                            let use_range = token.text_range();
                                            if use_range.start() > text_range.end() {
                                                let ty = local.ty(db);
                                                let ty_str =
                                                    ty.display(db, display_target).to_string();
                                                if seen_free.insert(name.clone()) {
                                                    output_variables.push(
                                                        rem_domain::ports::analysis::OutputVariable {
                                                            name,
                                                            ty: ty_str,
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    PathResolution::TypeParam(tp) => {
                                        let name = tp.name(db).as_str().to_string();
                                        if seen_generics.insert(name.clone()) {
                                            let bounds = tp.trait_bounds(db);
                                            let full_definition = if !bounds.is_empty() {
                                                let bounds_strs: Vec<String> = bounds
                                                    .iter()
                                                    .map(|b| b.name(db).as_str().to_string())
                                                    .collect();
                                                format!("{}: {}", name, bounds_strs.join(" + "))
                                            } else {
                                                name.clone()
                                            };
                                            referenced_generics
                                                .push(rem_domain::ports::analysis::GenericParam {
                                                    name,
                                                    full_definition,
                                                });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            // Don't traverse past the path itself
                        } else {
                            curr = node.parent();
                        }
                    }
                }
            }

            // ── 2. Detect control-flow exits ────────────────────────────────
            for node in syntax.descendants() {
                if !text_range.contains_range(node.text_range()) {
                    continue;
                }

                // Return expressions — only if not inside a closure in the selection
                if ast::ReturnExpr::cast(node.clone()).is_some() {
                    let is_in_closure = node
                        .ancestors()
                        .skip(1)
                        .take_while(|anc| text_range.contains_range(anc.text_range()))
                        .any(|anc| ast::ClosureExpr::cast(anc).is_some());
                    if !is_in_closure
                        && !control_flow_exits
                            .contains(&rem_domain::value_objects::ControlFlowKind::Return)
                    {
                        control_flow_exits
                            .push(rem_domain::value_objects::ControlFlowKind::Return);
                    }
                }

                // Break expressions — only if targeting a loop outside the selection
                if ast::BreakExpr::cast(node.clone()).is_some() {
                    let is_in_loop = node.ancestors().skip(1).any(|anc| {
                        if !text_range.contains_range(anc.text_range()) {
                            return false;
                        }
                        ast::LoopExpr::cast(anc.clone()).is_some()
                            || ast::ForExpr::cast(anc.clone()).is_some()
                            || ast::WhileExpr::cast(anc).is_some()
                    });
                    if !is_in_loop
                        && !control_flow_exits
                            .contains(&rem_domain::value_objects::ControlFlowKind::Break)
                    {
                        control_flow_exits
                            .push(rem_domain::value_objects::ControlFlowKind::Break);
                    }
                }

                // Continue expressions
                if ast::ContinueExpr::cast(node.clone()).is_some() {
                    let is_in_loop = node.ancestors().skip(1).any(|anc| {
                        if !text_range.contains_range(anc.text_range()) {
                            return false;
                        }
                        ast::LoopExpr::cast(anc.clone()).is_some()
                            || ast::ForExpr::cast(anc.clone()).is_some()
                            || ast::WhileExpr::cast(anc).is_some()
                    });
                    if !is_in_loop
                        && !control_flow_exits
                            .contains(&rem_domain::value_objects::ControlFlowKind::Continue)
                    {
                        control_flow_exits
                            .push(rem_domain::value_objects::ControlFlowKind::Continue);
                    }
                }

                // Try operator (?)
                if ast::TryExpr::cast(node.clone()).is_some()
                    && !control_flow_exits
                        .contains(&rem_domain::value_objects::ControlFlowKind::Try)
                {
                    control_flow_exits
                        .push(rem_domain::value_objects::ControlFlowKind::Try);
                }
            }

            // ── 3. Refine ownership based on actual usage in the selection ──
            refine_ownership(syntax, text_range, &mut free_variables);

            Ok(SelectionAnalysis {
                free_variables,
                output_variables,
                control_flow_exits,
                is_async,
                is_const,
                referenced_generics,
            })
        })
    }
}

fn resolve_file_id(vfs: &Vfs, file: &FilePath) -> Result<FileId, DomainError> {
    let abs = AbsPathBuf::try_from(file.as_str())
        .map_err(|_| DomainError::InvalidFilePath(file.as_str().into()))?;

    vfs.file_id(&abs.into())
        .map(|(id, _excluded)| id)
        .ok_or_else(|| {
            DomainError::InvalidFilePath(format!(
                "file not found in VFS: {}",
                file.as_str()
            ))
        })
}

/// Refine ownership of free variables based on how each is actually used
/// within the selection.
///
/// Rules (priority order):
///  - Any usage is an assignment target (`x = …`, `x += …`) → **MutRef**
///  - Any usage is behind `&mut` → **MutRef**
///  - Any usage is behind `&` → shared borrow (not a move)
///  - Any other usage → potential move → **Owned**
///  - If no non-borrow usage exists → **SharedRef**
fn refine_ownership(
    syntax: &ra_ap_syntax::SyntaxNode,
    text_range: ra_ap_syntax::TextRange,
    free_variables: &mut [rem_domain::ports::analysis::FreeVariable],
) {
    for var in free_variables.iter_mut() {
        let mut has_move_usage = false;

        for descendant in syntax.descendants() {
            if !text_range.contains_range(descendant.text_range()) {
                continue;
            }

            let Some(name_ref) = ast::NameRef::cast(descendant) else { continue };
            if name_ref.text() != var.name {
                continue;
            }

            // Check if behind & or &mut — walk up through Path → RefExpr
            if let Some(ref_kind) = get_ref_kind(&name_ref) {
                if ref_kind {
                    // &mut → mutation
                    var.ownership = rem_domain::value_objects::OwnershipKind::MutRef;
                    break;
                }
                // & → shared borrow, not a move
                continue;
            }

            // Check if it's an assignment target (x = …, x += …)
            if is_assignment_target(&name_ref) {
                var.ownership = rem_domain::value_objects::OwnershipKind::MutRef;
                break;
            }

            // Everything else is a potential move (ownership transfer)
            has_move_usage = true;
        }

        // Only overwrite if we haven't already set MutRef
        if var.ownership != rem_domain::value_objects::OwnershipKind::MutRef {
            var.ownership = if has_move_usage {
                rem_domain::value_objects::OwnershipKind::Owned
            } else {
                rem_domain::value_objects::OwnershipKind::SharedRef
            };
        }
    }
}

/// Returns `Some(true)` for `&mut expr`, `Some(false)` for `& expr`, `None` otherwise.
fn get_ref_kind(name_ref: &ast::NameRef) -> Option<bool> {
    for ancestor in name_ref.syntax().ancestors().skip(1).take(3) {
        if let Some(ref_expr) = ast::RefExpr::cast(ancestor.clone()) {
            return Some(ref_expr.mut_token().is_some());
        }
        // Stop at expression-statement boundary
        if ast::ExprStmt::cast(ancestor).is_some() {
            break;
        }
    }
    None
}

/// Check whether `name_ref` appears as the LHS of an assignment
/// (`x = …`, `x += …`, `x -= …`, etc.).
fn is_assignment_target(name_ref: &ast::NameRef) -> bool {
    for ancestor in name_ref.syntax().ancestors().skip(1).take(5) {
        if let Some(bin_expr) = ast::BinExpr::cast(ancestor.clone()) {
            if matches!(bin_expr.op_kind(), Some(ast::BinaryOp::Assignment { .. })) {
                return bin_expr.lhs().map_or(false, |lhs| {
                    lhs.syntax()
                        .text_range()
                        .contains_range(name_ref.syntax().text_range())
                });
            }
            break;
        }
        if ast::ExprStmt::cast(ancestor).is_some() {
            break;
        }
    }
    false
}
