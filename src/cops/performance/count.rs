use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Performance/Count";
pub static COP: Count = Count;
pub struct Count;

impl Cop for Count {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let counter = method_name(&call);
        if !matches!(counter, "count" | "length" | "size") || !send_args(&call).is_empty() {
            return;
        }
        let Some(selector_node) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        let selector = method_name(&selector_node);
        if !matches!(selector, "select" | "filter" | "find_all" | "reject") || !call_args(&selector_node).is_empty() {
            return;
        }
        // (block $(call _ ...) ...) with a body, or (call _ ... (:block_pass _)).
        let block = match selector_node.block() {
            Some(b) if b.as_block_node().is_some() => {
                let b = b.as_block_node().unwrap();
                if b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()) {
                    return;
                }
                Some(b)
            }
            Some(_) => None,
            None => return,
        };
        // eligible_node?: not the send of a block or a block's body; a block needs a body.
        if call.block().is_some_and(|b| b.as_block_node().is_some_and(|b| plain(&b))) {
            return;
        }
        if call.block().is_none()
            && helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_block_node()).is_some_and(|b| plain(&b))
        {
            return;
        }
        if block.as_ref().is_some_and(|b| b.body().is_none()) {
            return;
        }
        let src = cx.src;
        let end = send_range(&call).1;
        let sel = selector_node.message_loc().unwrap();
        // negate_reject
        let negation = if selector != "reject" {
            None
        } else if let Some(b) = &block {
            let body = b.body().unwrap();
            let (s, e) = match body.as_statements_node() {
                Some(st) => {
                    let last = st.body().iter().last().unwrap();
                    (last.location().start_offset(), last.location().end_offset())
                }
                None => body_range(&body),
            };
            Some(((s, e), format!("!({})", src.slice(s, e))))
        } else {
            // `node.receiver.loc.begin` (the `(`) and the block pass's child: RuboCop raises
            // without them, losing the offense.
            let Some(open) = selector_node.opening_loc() else { return };
            let pass = selector_node.block().unwrap().as_block_argument_node().unwrap();
            let Some(child) = pass.expression() else { return };
            let inline = match child.as_symbol_node().filter(|_| !is_parser_dsym(&child, cx.config.target_ruby)) {
                Some(sym) => format!(" {{ |element| !element.{} }}", String::from_utf8_lossy(sym.unescaped())),
                None => format!(" {{ !{}.call }}", node_text(&child)),
            };
            Some(((open.start_offset(), selector_node.location().end_offset()), inline))
        };
        let dot = call.call_operator_loc().map(|d| d.start_offset());
        let msg = format!("Use `count` instead of `{selector}...{counter}`.");
        cx.add_fix(sel.start_offset(), end, msg, |c| {
            if let Some(d) = dot {
                let opts = Surrounding { side: Side::Left, ..Surrounding::default() };
                let (rs, re) = range_with_surrounding_space(src.text, d, end, opts);
                c.remove(rs, re);
            }
            c.replace(sel.start_offset(), sel.end_offset(), "count");
            if let Some(((s, e), text)) = &negation {
                c.replace(*s, *e, text);
            }
        });
    }
}

fn plain(b: &ruby_prism::BlockNode) -> bool {
    !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}
