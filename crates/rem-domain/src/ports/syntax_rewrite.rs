/// **Port**: syntax-level rewriting of extracted function bodies.
///
/// Used to insert dereference operators (`*`) for variables that are passed
/// by reference across the extracted-function boundary.
///
/// Implementors: `rem-infrastructure::adapters::syntax_rewrite`.
pub trait SyntaxRewritePort: Send + Sync {
    /// Rewrite `body` by inserting explicit `*` dereference operators before
    /// every bare reference to a variable whose name appears in `ref_var_names`.
    fn rewrite_body_with_derefs(&self, body: &str, ref_var_names: &[String]) -> String;
}
