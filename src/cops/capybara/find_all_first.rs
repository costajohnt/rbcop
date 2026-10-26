use crate::prelude::*;

pub const NAME: &str = "Capybara/FindAllFirst";
pub static COP: FindAllFirst = FindAllFirst;
pub struct FindAllFirst;

impl Cop for FindAllFirst {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send _ :all _ ...) {:first | :[] (int 0)}), the `all` call being this node
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "all" {
            return;
        }
        let args = send_args(&call);
        if args.is_empty() || helpers::rspec::block_of(&call).is_some() {
            return;
        }
        let Some(pi) = helpers::parser_parent::parser_parent_index(cx.ancestors, cx.ancestors.len()) else { return };
        let Some(parent) = cx.ancestors[pi].as_call_node() else { return };
        if is_csend(&parent)
            || !parent.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset())
            || !match (method_name(&parent), &send_args(&parent)[..]) {
                ("first", []) => true,
                ("[]", [i]) => i.as_integer_node().is_some() && int_literal_value(i) == Some(0),
                _ => false,
            }
        {
            return;
        }
        // nil_sensitive_use?: inside `and` / `or`, the receiver of a `&.` call, or a condition
        if cx.ancestors[..pi].iter().any(helpers::parser_parent::is_operator_keyword) {
            return;
        }
        let pnode = &cx.ancestors[pi];
        let pstart = pnode.location().start_offset();
        let same = |n: Option<Node>| n.is_some_and(|n| n.location().start_offset() == pstart && n.location().end_offset() == pnode.location().end_offset());
        if let Some(gi) = helpers::parser_parent::parser_parent_index(cx.ancestors, pi) {
            let g = &cx.ancestors[gi];
            let sensitive = match kind::of(g) {
                kind::CALL_NODE => {
                    let c = g.as_call_node().unwrap();
                    // a block owned by `g` is not where we came from: the parser parent is the call itself
                    is_csend(&c) && same(c.receiver())
                }
                kind::IF_NODE => same(Some(g.as_if_node().unwrap().predicate())),
                kind::UNLESS_NODE => same(Some(g.as_unless_node().unwrap().predicate())),
                kind::WHILE_NODE => {
                    let w = g.as_while_node().unwrap();
                    !w.is_begin_modifier() && same(Some(w.predicate()))
                }
                kind::UNTIL_NODE => {
                    let u = g.as_until_node().unwrap();
                    !u.is_begin_modifier() && same(Some(u.predicate()))
                }
                _ => false,
            };
            if sensitive {
                return;
            }
        }
        let Some(sel) = call.message_loc() else { return };
        let Some(psel) = parent.message_loc() else { return };
        let selector = args.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", ");
        let (s, e) = (sel.start_offset(), psel.end_offset());
        let replacement = format!("first({selector})");
        cx.add_fix(s, e, format!("Use `first({selector})`."), |c| c.replace(s, e, &replacement));
    }
}
