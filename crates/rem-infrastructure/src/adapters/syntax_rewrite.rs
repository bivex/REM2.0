/// Adapter: syntax-level body rewriting → `SyntaxRewritePort`
///
/// Uses `ra_ap_syntax` to parse Rust fragments and insert explicit
/// dereference operators where needed.

use ra_ap_syntax::{ast, AstNode, SyntaxNode};

use rem_domain::ports::syntax_rewrite::SyntaxRewritePort;

pub struct SyntaxRewriteAdapter;

/// Walk ancestors from a node until we find one that casts to `T`.
/// Stops after 5 levels to avoid infinite loops.
fn ancestors_until<T: AstNode>(node: &ast::NameRef) -> Option<T> {
    let mut current: Option<SyntaxNode> = node.syntax().parent();
    let mut depth = 0;
    while let Some(n) = current {
        if let Some(t) = T::cast(n.clone()) {
            return Some(t);
        }
        depth += 1;
        if depth > 5 {
            return None;
        }
        current = n.parent();
    }
    None
}

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
                    // AST chain for `y.push(4)`:
                    //   MethodCallExpr → PathExpr → Path → PathSegment → NameRef("y")
                    // For `*y`:
                    //   PrefixExpr(*) → PathExpr → Path → PathSegment → NameRef("y")
                    // For `y` in `foo(y)`:
                    //   ArgList → CallExpr → PathExpr → Path → PathSegment → NameRef("y")
                    //
                    // Walk up to find the PathExpr, then check its parent.
                    let path_expr = ancestors_until::<ast::PathExpr>(&name_ref);
                    let parent_of_path_expr = path_expr.as_ref().and_then(|pe| pe.syntax().parent());

                    // Skip if this NameRef is the receiver of a method call —
                    // Rust auto-derefs for method calls, so adding * would
                    // produce `*y.push(4)` which is `*(y.push(4))`, wrong.
                    let is_method_receiver = parent_of_path_expr
                        .as_ref()
                        .is_some_and(|parent| ast::MethodCallExpr::cast(parent.clone()).is_some());

                    if is_method_receiver {
                        let range = name_ref.syntax().text_range();
                        result.push_str(&dummy_source[last_pos.into()..range.end().into()]);
                        last_pos = range.end();
                        continue;
                    }

                    let is_already_deref = parent_of_path_expr
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
