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
            let mut _names_defined_inside: std::collections::HashSet<String> = std::collections::HashSet::new();

            // ── Detect async/const context ──────────────────────────────────
            // Walk ancestors from the selection start to find the enclosing fn
            let pos = TextSize::from(range.start);
            let mut enclosing_fn_return_type: Option<String> = None;
            if let Some(fn_def) = syntax
                .token_at_offset(pos)
                .next()
                .and_then(|t| t.parent_ancestors().find_map(ast::Fn::cast))
            {
                is_async = fn_def.async_token().is_some();
                is_const = fn_def.const_token().is_some();

                // Resolve the enclosing function's return type
                if let Some(ret_type) = fn_def.ret_type() {
                    let ret_ty_str = ret_type.syntax().text().to_string();
                    // ret_ty_str is "-> Result<i32, String>" — extract just the type
                    let ty = ret_ty_str.trim_start_matches("->").trim();
                    if ty != "()" && !ty.is_empty() {
                        enclosing_fn_return_type = Some(ty.to_string());
                    }
                }
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
                                                // Heuristic: if the variable is declared with `mut`,
                                                // it is likely mutated inside the selection (e.g. via
                                                // method calls like vec.push()).  Pre-set to MutRef so
                                                // that refine_ownership() does not downgrade to Owned.
                                                let is_declared_mut =
                                                    source.source.value.clone().left().map_or(false, |pat| {
                                                        pat.mut_token().is_some()
                                                    });
                                                let ownership = if is_declared_mut {
                                                    rem_domain::value_objects::OwnershipKind::MutRef
                                                } else {
                                                    rem_domain::value_objects::OwnershipKind::SharedRef
                                                };

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
                                                    let displayed = ty.display(db, display_target).to_string();
                                                    if displayed == "{unknown}" {
                                                        infer_type_from_binding(&source, &name)
                                                            .unwrap_or(displayed)
                                                    } else {
                                                        displayed
                                                    }
                                                });

                                                free_variables
                                                    .push(rem_domain::ports::analysis::FreeVariable {
                                                        name,
                                                        ty: ty_str,
                                                        ownership,
                                                    });
                                            }
                                        } else {
                                            // Variable defined inside the selection — track it
                                            _names_defined_inside.insert(name.clone());
                                            // Output variable: defined inside, used after selection
                                            let use_range = token.text_range();
                                            if use_range.start() > text_range.end() {
                                                let ty = local.ty(db);
                                                let ty_str = {
                                                    let displayed = ty.display(db, display_target).to_string();
                                                    if displayed == "{unknown}" {
                                                        infer_type_from_binding(&source, &name)
                                                            .unwrap_or(displayed)
                                                    } else {
                                                        displayed
                                                    }
                                                };
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

            // ── 2b. Detect output variables via AST pattern scan ────────────────
            // Look for `let <name>` bindings inside the selection whose names
            // appear as IDENT tokens after the selection.
            {
                let full_text = syntax.text().to_string();
                let selection_text = &full_text[text_range.start().into()..text_range.end().into()];
                let mut candidates: std::collections::HashSet<String> = std::collections::HashSet::new();

                // Parse just the selection as statements to find let bindings
                let sel_parse = ra_ap_syntax::SourceFile::parse(
                    &format!("fn _wrap() {{\n{selection_text}\n}}"),
                    ra_ap_syntax::Edition::Edition2021,
                );
                for node in sel_parse.tree().syntax().descendants() {
                    if let Some(let_stmt) = ast::LetStmt::cast(node) {
                        if let Some(pat) = let_stmt.pat() {
                            // Extract variable names from the pattern
                            for ident in pat.syntax().descendants() {
                                if let Some(name) = ast::Name::cast(ident) {
                                    let n = name.text().to_string();
                                    // Skip if already a free variable
                                    if !seen_free.contains(&n) {
                                        candidates.insert(n);
                                    }
                                }
                            }
                        }
                    }
                }

                // Now check which candidates appear after the selection
                for element in syntax
                    .descendants_with_tokens()
                    .filter(|el| el.text_range().start() >= text_range.end())
                {
                    let Some(token) = element.into_token() else { continue };
                    if token.kind() != ra_ap_syntax::SyntaxKind::IDENT {
                        continue;
                    }
                    let token_text = token.text().to_string();
                    if !candidates.contains(&token_text) {
                        continue;
                    }
                    if seen_free.contains(&token_text) {
                        continue;
                    }

                    // Found an output variable! Try to get its type.
                    let mut ty_str = "_".to_string();
                    for expanded in sema.descend_into_macros(token.clone()) {
                        let mut curr = Some(expanded.parent().expect("token must have a parent"));
                        while let Some(node) = curr.take() {
                            if let Some(path) = ast::Path::cast(node.clone()) {
                                if let Some(res) = sema.resolve_path(&path) {
                                    if let PathResolution::Local(local) = res {
                                        let ty = local.ty(db);
                                        let displayed = ty.display(db, display_target).to_string();
                                        if displayed != "{unknown}" {
                                            ty_str = displayed;
                                        } else {
                                            // Try to infer from the let binding inside selection
                                            let sources = local.sources(db);
                                            if let Some(src) = sources.first() {
                                                ty_str = infer_type_from_binding(&src, &token_text)
                                                    .unwrap_or_else(|| "usize".to_string());
                                            }
                                        }
                                    }
                                }
                                break;
                            } else {
                                curr = node.parent();
                            }
                        }
                        if ty_str != "_" {
                            break;
                        }
                    }

                    // If type is still unknown, try a simple heuristic from the selection text
                    if ty_str == "_" {
                        let let_pat = format!("let {} =", token_text);
                        if let Some(idx) = selection_text.find(&let_pat) {
                            let rest = &selection_text[idx + let_pat.len()..];
                            let rest = rest.trim();
                            if rest.contains(".len()") {
                                ty_str = "usize".to_string();
                            }
                        }
                    }

                    if seen_free.insert(token_text.clone()) {
                        output_variables.push(
                            rem_domain::ports::analysis::OutputVariable {
                                name: token_text,
                                ty: ty_str,
                            },
                        );
                    }
                }
            }

            // ── 3. Refine ownership based on actual usage in the selection ──
            // Run BEFORE the text-based fallback so that text-scan variables
            // keep their pre-set ownership.
            refine_ownership(syntax, text_range, &mut free_variables);

            // ── 2c. Text-based fallback for free variables missed by sema ──────
            // When proc-macro expansion is disabled, tokens inside macros (like
            // format!("... {}", i)) don't resolve. Do a simple text scan.
            {
                let keywords: std::collections::HashSet<&str> = [
                    "fn", "let", "if", "else", "match", "for", "while", "loop",
                    "return", "break", "continue", "struct", "enum", "impl", "pub",
                    "mut", "ref", "self", "super", "crate", "mod", "use", "as",
                    "in", "where", "type", "const", "static", "true", "false",
                    "Some", "None", "Ok", "Err", "async", "await", "move",
                ].into_iter().collect();

                let full_text = syntax.text().to_string();
                let sel_text = &full_text[text_range.start().into()..text_range.end().into()];

                for word in simple_ident_scan(sel_text) {
                    if keywords.contains(word.as_str()) { continue; }
                    if seen_free.contains(&word) { continue; }

                    // Check if this word appears to be a variable reference by
                    // looking for it in the enclosing scope (before the selection).
                    // Use word-boundary matching to avoid false positives.
                    let before_sel = &full_text[..text_range.start().into()];
                    if !contains_as_ident(before_sel, &word) {
                        continue;
                    }
                    if contains_as_ident(before_sel, &word) {
                        tracing::info!(word=%word, "2c: text-based free var found");
                        // Likely a free variable that sema missed
                        // Try to get the type from context
                        let ty = guess_type_from_context(before_sel, &word)
                            .unwrap_or_else(|| format!("__{}", word));
                        tracing::info!(word=%word, ty=%ty, "2c: guessed type");

                        // Copy types should be passed by value, not by reference
                        let ownership = if ty.starts_with("i32") || ty.starts_with("u")
                            || ty.starts_with("f") || ty == "bool" || ty == "usize"
                            || ty.starts_with("__") // unknown generic — use Owned
                        {
                            rem_domain::value_objects::OwnershipKind::Owned
                        } else {
                            rem_domain::value_objects::OwnershipKind::SharedRef
                        };

                        seen_free.insert(word.clone());
                        free_variables.push(rem_domain::ports::analysis::FreeVariable {
                            name: word,
                            ty,
                            ownership,
                        });
                    }
                }
            }

            // ── 2d. Fix remaining {unknown} types with context guesses ────────
            {
                let full_text = syntax.text().to_string();
                let before_sel = &full_text[..text_range.start().into()];
                for var in free_variables.iter_mut() {
                    if var.ty == "{unknown}" {
                        if let Some(guessed) = guess_type_from_context(before_sel, &var.name) {
                            var.ty = guessed;
                        }
                    }
                }
            }

            Ok(SelectionAnalysis {
                free_variables,
                output_variables,
                control_flow_exits,
                is_async,
                is_const,
                referenced_generics,
                enclosing_fn_return_type,
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

/// When rust-analyzer returns `{unknown}` for a type, try to infer it from
/// the `let` binding's initializer expression.
fn infer_type_from_binding(
    source: &ra_ap_hir::LocalSource,
    var_name: &str,
) -> Option<String> {
    let pat = source.source.value.clone().left()?;
    // Find the let statement that contains this pattern
    let let_stmt = pat.syntax().parent().and_then(ast::LetStmt::cast)?;
    let init = let_stmt.initializer()?;

    let init_text = init.syntax().text().to_string();

    // Simple heuristic mapping from initializer syntax to type
    let inferred = if init_text.starts_with("vec![") || init_text.starts_with("Vec::") {
        // Try to extract element type from vec![1, 2, 3] → Vec<i32>
        if let Some(inner) = init_text.strip_prefix("vec![").and_then(|s| s.strip_suffix(']')) {
            let first = inner.split(',').next().unwrap_or("").trim();
            if first.parse::<i32>().is_ok() {
                "Vec<i32>".to_string()
            } else if first.parse::<f64>().is_ok() {
                "Vec<f64>".to_string()
            } else if first.starts_with('"') {
                "Vec<String>".to_string()
            } else {
                "Vec<_>".to_string()
            }
        } else {
            "Vec<_>".to_string()
        }
    } else if init_text.starts_with("String::from(") || init_text.starts_with("format!(") {
        "String".to_string()
    } else if init_text.starts_with('"') {
        "&str".to_string()
    } else if init_text.parse::<i32>().is_ok() {
        "i32".to_string()
    } else if init_text.parse::<f64>().is_ok() {
        "f64".to_string()
    } else if init_text.parse::<bool>().is_ok() {
        "bool".to_string()
    } else if init_text.starts_with("Ok(") || init_text.starts_with("Err(") {
        // Can't easily determine the full Result type
        return None;
    } else {
        // Check for method chains that reveal the type
        if init_text.contains(".len()") || init_text.contains(".push(") {
            return None; // Too ambiguous
        }
        return None;
    };

    Some(inferred)
}

/// Simple text-based scan for identifiers that might be variable references.
/// Skips string literals, comments, and method/function calls (foo.bar, foo!).
fn simple_ident_scan(text: &str) -> Vec<String> {
    let mut idents = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut in_string = false;

    while i < bytes.len() {
        if in_string {
            if bytes[i] == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // Skip comment
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];

            // Skip if followed by `(` (function call), `!` (macro), or preceded by `.` (method call)
            let after = if i < bytes.len() { bytes[i] } else { b' ' };
            let before = if start > 0 { bytes[start - 1] } else { b' ' };
            if after == b'(' || after == b'!' || before == b'.' {
                continue;
            }

            if !word.starts_with('_') && seen.insert(word.to_string()) {
                idents.push(word.to_string());
            }
        } else {
            i += 1;
        }
    }
    idents
}

/// Check if `name` appears as a standalone identifier in `text`.
/// Uses word-boundary matching to avoid matching substrings.
fn contains_as_ident(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len();

    for i in 0..=bytes.len().saturating_sub(name_len) {
        if &bytes[i..i + name_len] == name_bytes {
            // Check word boundaries
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_';
            let after_ok = i + name_len >= bytes.len() || !bytes[i + name_len].is_ascii_alphanumeric() && bytes[i + name_len] != b'_';
            if before_ok && after_ok {
                return true;
            }
        }
    }
    false
}

/// Try to guess the type of a variable from its usage context in the text before the selection.
fn guess_type_from_context(before_sel: &str, name: &str) -> Option<String> {
    // Look for `for (<name>,` or `for (<other>, <name>` patterns
    let for_pat1 = format!("for ({},", name);
    let for_pat2 = format!(", {})", name);
    let for_pat3 = format!("for ({},", name);

    if before_sel.contains(&for_pat1) || before_sel.contains(&for_pat3) {
        // First element of for tuple — likely a usize index from enumerate()
        if before_sel.contains(".enumerate()") {
            return Some("usize".to_string());
        }
    }
    if before_sel.contains(&for_pat2) {
        // Second element of for tuple
        if before_sel.contains(".enumerate()") {
            // Look at the collection type: Vec<Option<i32>>
            if let Some(vec_start) = before_sel.find("Vec<") {
                let inner = &before_sel[vec_start + 4..];
                // Find the matching '>' — need to handle nested generics
                let mut depth = 1;
                let mut end = 0;
                for (i, c) in inner.chars().enumerate() {
                    match c {
                        '<' => depth += 1,
                        '>' => { depth -= 1; if depth == 0 { end = i; break; } }
                        _ => {}
                    }
                }
                if end > 0 {
                    return Some(inner[..end].to_string());
                }
            }
        }
    }

    None
}
