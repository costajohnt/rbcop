use crate::helpers::parser_parent::is_top_const;
use crate::helpers::sorbet_rbs_assertion::{descendants, no_literal_block};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/CallbackConditionalsBinding";
pub static COP: CallbackConditionalsBinding = CallbackConditionalsBinding;
pub struct CallbackConditionalsBinding;

#[rustfmt::skip]
const CALLBACKS: &[&str] = &[
    "validate", "validates", "validates_with", "before_validation", "around_validation", "before_create",
    "before_save", "before_destroy", "before_update", "after_create", "after_save", "after_destroy",
    "after_update", "after_touch", "after_initialize", "after_find", "around_create", "around_save",
    "around_destroy", "around_update", "before_commit", "after_commit", "after_create_commit",
    "after_destroy_commit", "after_rollback", "after_save_commit", "after_update_commit", "before_action",
    "prepend_before_action", "append_before_action", "around_action", "prepend_around_action",
    "append_around_action", "after_action", "prepend_after_action", "append_after_action",
];

/// `(node.parent&.begin_type? ? node.parent.parent : node.parent)&.defined_module_name`, for the
/// parents that can hold a send: a class or module (a casgn's value would have to be `Class.new`).
fn enclosing_module_name(ancestors: &[Node]) -> Option<String> {
    let parents: Vec<usize> = parser_ancestor_ranges(ancestors).into_iter().map(|(i, _, _)| i).take(2).collect();
    let begin = |n: &Node| matches!(n, Node::StatementsNode { .. } | Node::ParenthesesNode { .. });
    let i = match parents.as_slice() {
        [p, rest @ ..] if begin(&ancestors[*p]) => *rest.first()?,
        [p, ..] => *p,
        [] => return None,
    };
    let path = match &ancestors[i] {
        Node::ClassNode { .. } => ancestors[i].as_class_node().unwrap().constant_path(),
        Node::ModuleNode { .. } => ancestors[i].as_module_node().unwrap().constant_path(),
        _ => return None,
    };
    helpers::const_name::const_name(&path)
}

/// `` `(send (const {cbase nil?} :T) :bind self $_) ``
fn binds_self(body: &Node) -> bool {
    descendants(body).iter().any(|n| {
        n.as_call_node().is_some_and(|c| {
            method_name(&c) == "bind"
                && !is_csend(&c)
                && no_literal_block(&c)
                && c.receiver().is_some_and(|r| is_top_const(&r, "T"))
                && matches!(send_args(&c).as_slice(), [s, _] if s.as_self_node().is_some())
        })
    })
}

impl Cop for CallbackConditionalsBinding {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !CALLBACKS.contains(&method_name(&call)) || is_csend(&call) || !no_literal_block(&call) {
            return;
        }
        let Some(ty) = enclosing_module_name(cx.ancestors) else { return };
        let src = cx.src;
        let width = helpers::alignment::configured_indentation_width(cx).max(0) as usize;
        for arg in send_args(&call) {
            let elements: Vec<Node> = match &arg {
                Node::HashNode { .. } => arg.as_hash_node().unwrap().elements().iter().collect(),
                Node::KeywordHashNode { .. } => arg.as_keyword_hash_node().unwrap().elements().iter().collect(),
                _ => continue,
            };
            for pair in elements {
                let Some(assoc) = pair.as_assoc_node() else { continue };
                if !assoc.key().as_symbol_node().is_some_and(|s| matches!(s.unescaped(), b"if" | b"unless")) {
                    continue;
                }
                // $(block (send nil? {:lambda :proc}) (args) !`(send (const {cbase nil?} :T) :bind self $_))
                // (a `->` literal matches `(send nil? :lambda)` too)
                let value = assoc.value();
                let (params, body, opening, closing) = if let Some(l) = value.as_lambda_node() {
                    (l.parameters(), l.body(), l.opening_loc(), l.closing_loc())
                } else {
                    let Some(callable) = value.as_call_node() else { continue };
                    if callable.receiver().is_some()
                        || is_csend(&callable)
                        || callable.arguments().is_some()
                        || !matches!(method_name(&callable), "lambda" | "proc")
                    {
                        continue;
                    }
                    let Some(block) = callable.block().and_then(|b| b.as_block_node()) else { continue };
                    (block.parameters(), block.body(), block.opening_loc(), block.closing_loc())
                };
                let no_args = match params {
                    None => true,
                    Some(p) => p
                        .as_block_parameters_node()
                        .is_some_and(|p| p.parameters().is_none() && p.locals().iter().next().is_none()),
                };
                // An empty body crashes the corrector (`block.body.source_range`): no offense.
                let Some(body) = body else { continue };
                if !no_args || binds_self(&body) {
                    continue;
                }
                let (bs, be) = body_range(&body);
                let block_start = value.location().start_offset();
                let line = src.lines[src.line_of(block_start) - 1];
                let open_indent: String = line.chars().take_while(|&c| c == ' ').collect();
                let body_indent = format!("{open_indent}{}", " ".repeat(width));
                let (open_end, close_start) = (opening.end_offset(), closing.start_offset());
                let single_line = src.line_of(opening.start_offset()) == src.line_of(close_start);
                let insertion = format!("T.bind(self, {ty})\n{body_indent}");
                let (ps, pe) = helpers::node_types::range(&pair);
                cx.add_fix(ps, pe, format!("Callback conditionals should be bound to the right type. Use T.bind(self, {ty})"), |c| {
                    if single_line {
                        c.replace(open_end, bs, &format!("\n{body_indent}"));
                        c.replace(be, close_start, &format!("\n{open_indent}"));
                    }
                    c.insert_before(bs, be, &insertion);
                });
            }
        }
    }
}
