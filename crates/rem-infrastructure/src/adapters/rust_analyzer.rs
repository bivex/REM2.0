/// Adapter: rust-analyzer → `CodeAnalysisPort`
///
/// Uses the `ra-ap-*` public API crates to load a Cargo workspace into
/// memory and answer semantic questions about selected fragments without
/// spawning `rustc` as a subprocess.

use std::sync::Mutex;

use ra_ap_ide::{AnalysisHost, FileId, TextRange, TextSize};
use ra_ap_load_cargo::{LoadCargoConfig, ProcMacroServerChoice, load_workspace_at};
use ra_ap_project_model::CargoConfig;
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
    vfs:  Vfs,
}

impl RustAnalyzerAdapter {
    pub fn new() -> Self {
        Self { inner: Mutex::new(None) }
    }
}

impl Default for RustAnalyzerAdapter {
    fn default() -> Self { Self::new() }
}

impl CodeAnalysisPort for RustAnalyzerAdapter {
    fn load_workspace(&self, project_root: &str) -> Result<(), DomainError> {
        use std::path::Path;

        let root = Path::new(project_root);
        let cargo_cfg = CargoConfig::default();
        let load_cfg = LoadCargoConfig {
            load_out_dirs_from_check:  false,
            with_proc_macro_server:    ProcMacroServerChoice::None,
            prefill_caches:            false,
            num_worker_threads:        0,          // 0 = use rayon default
            proc_macro_processes:      Default::default(),
        };

        let (db, vfs, _proc_macro) =
            load_workspace_at(root, &cargo_cfg, &load_cfg, &|_| {})
                .map_err(|e| DomainError::InvalidFilePath(format!(
                    "workspace load failed at `{project_root}`: {e}"
                )))?;

        // `load_workspace_at` returns `RootDatabase` directly in 0.0.328.
        // Wrap it in an `AnalysisHost` so we can call `.analysis()`.
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

        let file_id  = resolve_file_id(&inner.vfs, file)?;

        let text_range = TextRange::new(
            TextSize::from(range.start),
            TextSize::from(range.end),
        );

        let db = inner.host.raw_database();
        let analysis = inner.host.analysis();

        let mut free_variables = Vec::new();
        let mut output_variables = Vec::new();
        let mut control_flow_exits = Vec::new();
        let mut seen_free = std::collections::HashSet::new();
        let mut seen_out = std::collections::HashSet::new();

        use ra_ap_syntax::{ast, AstNode};
        use ra_ap_syntax::ast::HasName;
        use ra_ap_ide::{FilePosition, GotoDefinitionConfig, FindAllRefsConfig, RaFixtureConfig};
        use ra_ap_hir::{PathResolution, Semantics, HirDisplay, DisplayTarget};

        let _goto_config = GotoDefinitionConfig {
            ra_fixture: RaFixtureConfig::default(),
        };
        let refs_config = FindAllRefsConfig {
            search_scope: None,
            ra_fixture: RaFixtureConfig::default(),
        };

        let sema = Semantics::new(db);
        let editioned_file_id = sema.attach_first_edition(file_id);
        let source_file = sema.parse(editioned_file_id);
        let syntax = source_file.syntax();
        
        let krate = sema.first_crate(file_id).unwrap_or_else(|| {
            ra_ap_hir::Crate::all(db).into_iter().next().expect("no crates in database")
        });
        
        let display_target = DisplayTarget::from_crate(db, krate.into());

        // 1. Find free variables (used inside, defined outside)
        let inside_name_refs: Vec<ast::NameRef> = syntax
            .descendants()
            .filter(|n| text_range.contains_range(n.text_range()))
            .filter_map(ast::NameRef::cast)
            .collect();

        for name_ref in inside_name_refs {
            ra_ap_hir::attach_db(db, || {
                let path = name_ref.syntax().ancestors().find_map(ast::Path::cast)?;
                let resolution = sema.resolve_path(&path)?;
                if let PathResolution::Local(local) = resolution {
                    let sources = local.sources(db);
                    let source = sources.first().unwrap();
                    let def_range = source.source.value.syntax().text_range();
                    if !text_range.contains_range(def_range) {
                        let name = name_ref.to_string();
                        if seen_free.insert(name.clone()) {
                            let mut ownership = rem_domain::value_objects::OwnershipKind::SharedRef;
                            if local.is_mut(db) {
                                ownership = rem_domain::value_objects::OwnershipKind::MutRef;
                            }
                            let ty_str = local.ty(db).display(db, display_target).to_string();

                            free_variables.push(rem_domain::ports::analysis::FreeVariable {
                                name,
                                ty: ty_str,
                                ownership,
                            });
                        }
                    }
                }
                Some(())
            });
        }

        // 2. Find output variables (defined inside, used after)
        let inside_idents: Vec<ast::IdentPat> = syntax
            .descendants()
            .filter(|n| text_range.contains_range(n.text_range()))
            .filter_map(ast::IdentPat::cast)
            .collect();

        for ident in inside_idents {
            if let Some(name) = ident.name() {
                let offset = name.syntax().text_range().start();
                let pos = FilePosition { file_id, offset };
                if let Ok(Some(results)) = analysis.find_all_refs(pos, &refs_config) {
                    let mut used_after = false;
                    for res in results {
                        for (ref_file_id, refs) in res.references {
                            if ref_file_id == file_id {
                                if refs.iter().any(|(range, _cat)| range.start() >= text_range.end()) {
                                    used_after = true;
                                    break;
                                }
                            }
                        }
                        if used_after { break; }
                    }

                    if used_after {
                        let name_str = name.to_string();
                        if seen_out.insert(name_str.clone()) {
                            let ty_str = ra_ap_hir::attach_db(db, || {
                                sema.to_def(&ident).map(|local| local.ty(db).display(db, display_target).to_string())
                            }).unwrap_or_else(|| String::from("_"));
                            
                            output_variables.push(rem_domain::ports::analysis::OutputVariable {
                                name: name_str,
                                ty: ty_str,
                            });
                        }
                    }
                }
            }
        }

        // 3. Control Flow Exits
        for node in syntax.descendants().filter(|n| text_range.contains_range(n.text_range())) {
            if let Some(_re) = ast::ReturnExpr::cast(node.clone()) {
                control_flow_exits.push(rem_domain::value_objects::ControlFlowKind::Return);
            } else if let Some(_be) = ast::BreakExpr::cast(node.clone()) {
                // Heuristic: for now just ignore breaks inside the selection,
                // as a truly robust check requires loop target resolution.
            } else if let Some(_ce) = ast::ContinueExpr::cast(node.clone()) {
                // Heuristic: same for continue.
            }
        }
        control_flow_exits.dedup();

        let is_async = syntax
            .descendants()
            .filter(|n| text_range.contains_range(n.text_range()))
            .any(|n| ast::AwaitExpr::can_cast(n.kind()));

        let res = SelectionAnalysis {
            free_variables,
            output_variables,
            control_flow_exits,
            is_async,
            is_const: false,
            referenced_generics: vec![],
        };
        
        tracing::info!(?res.free_variables, ?res.output_variables, ?res.control_flow_exits, "Analysis result");
        
        Ok(res)
    }
}

fn resolve_file_id(vfs: &Vfs, file: &FilePath) -> Result<FileId, DomainError> {
    let abs = AbsPathBuf::try_from(file.as_str())
        .map_err(|_| DomainError::InvalidFilePath(file.as_str().into()))?;

    // In ra_ap_vfs 0.0.328, file_id returns Option<(FileId, FileExcluded)>.
    vfs.file_id(&abs.into())
        .map(|(id, _excluded)| id)
        .ok_or_else(|| DomainError::InvalidFilePath(format!(
            "file not found in VFS: {}", file.as_str()
        )))
}
