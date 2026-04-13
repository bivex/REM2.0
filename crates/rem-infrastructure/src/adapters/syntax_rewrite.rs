/// Adapter: syntax-level body rewriting → `SyntaxRewritePort`
///
/// Uses `ra_ap_syntax` to parse Rust fragments and insert explicit
/// dereference operators where needed.

use ra_ap_syntax::{ast, AstNode};

use rem_domain::ports::syntax_rewrite::SyntaxRewritePort;

pub struct SyntaxRewriteAdapter;

impl SyntaxRewritePort for SyntaxRewriteAdapter {
    fn rewrite_body_with_derefs(&self, body: &str, ref_var_names: &[String]) -> String {
        let refs_to_deref: std::collections::HashSet<&str> =
            ref_var_names.iter().map(|s| s.as_str()).collect();

        if refs_to_deref.is_empty() {
            return body.to_string();
        }

        // Wrap in a dummy function to get a valid parse tree.
        let dummy_source = format!("fn _dummy() {{\n{body}\n}}");
        let parse =
            ra_ap_syntax::SourceFile::parse(&dummy_source, ra_ap_syntax::Edition::Edition2021);
        let root = parse.tree();

        let dummy_fn = root
            .syntax()
            .descendants()
            .find_map(ast::Fn::cast)
            .expect("dummy fn not found");
        let body_node = dummy_fn.body().expect("dummy body not found");
        let body_range = body_node.syntax().text_range();

        let mut result = String::new();
        let mut last_pos = body_range.start() + ra_ap_syntax::TextSize::from(1); // skip '{'
        let end_pos = body_range.end() - ra_ap_syntax::TextSize::from(1); // skip '}'

        for node in body_node.syntax().descendants() {
            if let Some(name_ref) = ast::NameRef::cast(node.clone()) {
                let name_str = name_ref.to_string();
                if refs_to_deref.contains(name_str.as_str()) {
                    let is_already_deref = name_ref
                        .syntax()
                        .parent()
                        .and_then(ast::Path::cast)
                        .and_then(|p| p.syntax().parent())
                        .and_then(ast::PrefixExpr::cast)
                        .map(|pe| pe.op_kind() == Some(ast::UnaryOp::Deref))
                        .unwrap_or(false);

                    if !is_already_deref {
                        let range = name_ref.syntax().text_range();
                        result.push_str(&dummy_source[last_pos.into()..range.start().into()]);
                        result.push('*');
                        result.push_str(&dummy_source[range.start().into()..range.end().into()]);
                        last_pos = range.end();
                    }
                }
            }
        }

        result.push_str(&dummy_source[last_pos.into()..end_pos.into()]);

        result
            .trim_matches(|c: char| c == '\n' || c == ' ')
            .to_string()
    }
}
