use crate::prelude::*;

pub const NAME: &str = "Layout/AccessModifierIndentation";
pub static COP: AccessModifierIndentation = AccessModifierIndentation;
pub struct AccessModifierIndentation;

/// `bare_access_modifier_declaration?` (scope is checked by the caller).
fn bare_modifier(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        c.receiver().is_none()
            && c.arguments().is_none()
            && c.block().is_none()
            && matches!(method_name(&c), "public" | "protected" | "private" | "module_function")
    })
}

/// `effective_column`: column, minus one on line 1 after a byte order mark.
fn effective_column(src: &crate::source::Source, off: usize) -> i64 {
    let col = src.column(off) as i64;
    if src.line_of(off) == 1 && src.text.starts_with('\u{feff}') { col - 1 } else { col }
}

impl Cop for AccessModifierIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE, kind::SINGLETON_CLASS_NODE, kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (body, start, end_loc) = match kind::of(node) {
            kind::CLASS_NODE => {
                let n = node.as_class_node().unwrap();
                (n.body(), node.location().start_offset(), n.end_keyword_loc())
            }
            kind::MODULE_NODE => {
                let n = node.as_module_node().unwrap();
                (n.body(), node.location().start_offset(), n.end_keyword_loc())
            }
            kind::SINGLETON_CLASS_NODE => {
                let n = node.as_singleton_class_node().unwrap();
                (n.body(), node.location().start_offset(), n.end_keyword_loc())
            }
            kind::BLOCK_NODE => {
                let n = node.as_block_node().unwrap();
                // on_block only: numbered/`it` blocks are numblock/itblock.
                if n.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
                    return;
                }
                let start = cx.parent().map_or(node.location().start_offset(), |p| p.location().start_offset());
                (n.body(), start, n.closing_loc())
            }
            _ => {
                let n = node.as_lambda_node().unwrap();
                if n.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
                    return;
                }
                (n.body(), node.location().start_offset(), n.closing_loc())
            }
        };
        // body.begin_type?: two or more statements, no rescue/ensure wrapper.
        let Some(stmts) = body.and_then(|b| b.as_statements_node()) else { return };
        if stmts.body().iter().count() < 2 {
            return;
        }
        // Blocks: the modifier is a macro only if the block itself is in a macro scope.
        let block_scope = match kind::of(node) {
            kind::BLOCK_NODE => {
                cx.parent().is_some_and(helpers::macro_scope_nodes::class_constructor)
                    || helpers::macro_scope_nodes::in_macro_scope(&cx.ancestors[..cx.ancestors.len().saturating_sub(1)])
            }
            kind::LAMBDA_NODE => helpers::macro_scope_nodes::in_macro_scope(cx.ancestors),
            _ => true,
        };
        if !block_scope {
            return;
        }
        let src = cx.src;
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("indent").to_string();
        let expected = if style == "outdent" { 0 } else { helpers::alignment::configured_indentation_width(cx) };
        let end_col = effective_column(src, end_loc.start_offset());
        let node_line = src.line_of(start);
        let label = if style == "outdent" { "Outdent" } else { "Indent" };
        for m in stmts.body().iter().filter(bare_modifier) {
            let loc = m.location();
            if src.line_of(loc.start_offset()) == node_line {
                continue;
            }
            let delta = expected - (effective_column(src, loc.start_offset()) - end_col);
            if delta != 0 {
                let correctable = helpers::alignment::corrects(cx, loc.start_offset(), loc.end_offset(), delta);
                cx.add_with(
                    loc.start_offset(),
                    loc.end_offset(),
                    format!("{label} access modifiers like `{}`.", node_text(&m)),
                    correctable,
                );
            }
        }
    }
}
