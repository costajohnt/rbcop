use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::helpers::sorbet_rbs_assertion::{descendants, no_literal_block};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/BlockMethodDefinition";
pub static COP: BlockMethodDefinition = BlockMethodDefinition;
pub struct BlockMethodDefinition;

const MSG: &str = "Do not define methods in blocks (use `define_method` as a workaround).";

/// `(module _ (begin <(send nil? :extend (const (const {nil? cbase} :ActiveSupport) :Concern)) ...> ...))`
fn extends_concern(module: &ruby_prism::ModuleNode) -> bool {
    let Some(stmts) = module.body().and_then(|b| b.as_statements_node()) else { return false };
    let body: Vec<Node> = stmts.body().iter().collect();
    body.len() >= 2
        && body.iter().any(|n| {
            n.as_call_node().is_some_and(|c| {
                method_name(&c) == "extend"
                    && c.receiver().is_none()
                    && !is_csend(&c)
                    && no_literal_block(&c)
                    && matches!(send_args(&c).as_slice(), [a] if a.as_constant_path_node().is_some_and(|p| {
                        p.name().is_some_and(|n| n.as_slice() == b"Concern")
                            && p.parent().is_some_and(|s| is_top_const(&s, "ActiveSupport"))
                    }))
            })
        })
}

/// The replacement of a def's signature (`define_method(:name) do |args|` plus what follows) and
/// the end of the range it replaces.
fn correction(src: &crate::source::Source, def: &ruby_prism::DefNode) -> Option<(usize, String)> {
    let loc = def.location();
    let indent = " ".repeat(src.column(loc.start_offset()));
    let name = String::from_utf8_lossy(def.name().as_slice());
    let params = def.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
    let args = if params.is_empty() {
        String::new()
    } else {
        format!(" |{}|", params.iter().map(node_text).collect::<Vec<_>>().join(", "))
    };
    let signature = match def.receiver() {
        None => format!("define_method(:{name}) do{args}"),
        // parser's defs receiver for `def (expr).name` is `expr`, without the parentheses
        Some(r) => {
            let r = strip_parens(r)?;
            format!("{}.define_singleton_method(:{name}) do{args}", node_text(&r))
        }
    };
    if let Some(body) = def.body() {
        return Some((body_range(&body).0, format!("{signature}\n{indent}  ")));
    }
    let source = src.slice(loc.start_offset(), loc.end_offset());
    if !source.contains('\n') {
        return Some((loc.end_offset(), format!("{signature}\n{indent}end")));
    }
    let end = match params.last() {
        Some(last) => {
            let end = last.location().end_offset();
            // `/\A\s*\)/` after the last argument
            let rest = &src.text[end..];
            let ws = rest.len() - rest.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']).len();
            if rest[ws..].starts_with(')') { end + ws + 1 } else { end }
        }
        None => def.name_loc().end_offset(),
    };
    Some((end, signature))
}

impl Cop for BlockMethodDefinition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let block = helpers::node_types::literal_block(node);
        if block.is_none() && node.as_lambda_node().is_none() {
            return;
        }
        if parser_parent(cx.ancestors).is_some_and(|p| matches!(p, Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. })) {
            return;
        }
        // in_activesupport_concern_class_methods_block?: `(block (send nil? :class_methods) _ _)`
        // in the nearest module, which extends ActiveSupport::Concern.
        if let (Some(b), Some(call)) = (&block, node.as_call_node())
            && method_name(&call) == "class_methods"
            && call.receiver().is_none()
            && !is_csend(&call)
            && call.arguments().is_none()
            && helpers::node_types::plain_block_params(b.parameters())
            && cx.ancestors.iter().rev().find_map(|a| a.as_module_node()).is_some_and(|m| extends_concern(&m))
        {
            return;
        }
        let src = cx.src;
        for d in descendants(node) {
            let Some(def) = d.as_def_node() else { continue };
            let fix = correction(src, &def);
            let (s, e) = helpers::node_types::range(&d);
            cx.add_fix(s, e, MSG, |c| {
                if let Some((end, replacement)) = &fix {
                    c.replace(s, *end, replacement);
                }
            });
        }
    }
}
