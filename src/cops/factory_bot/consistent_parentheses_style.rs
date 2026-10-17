use crate::prelude::*;

pub const NAME: &str = "FactoryBot/ConsistentParenthesesStyle";
pub static COP: ConsistentParenthesesStyle = ConsistentParenthesesStyle;
pub struct ConsistentParenthesesStyle;

impl Cop for ConsistentParenthesesStyle {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !helpers::factory_bot::METHODS.contains(&name) {
            return;
        }
        // ambiguous_without_parentheses?: a send, pair, array, and, or or if parent
        if helpers::rspec::block_of(&call).is_none()
            && let Some(p) = helpers::parser_parent::parser_parent(cx.ancestors)
        {
            let ambiguous = match kind::of(p) {
                kind::CALL_NODE => !is_csend(&p.as_call_node().unwrap()),
                kind::ASSOC_NODE | kind::ARRAY_NODE | kind::AND_NODE | kind::OR_NODE | kind::IF_NODE | kind::UNLESS_NODE => true,
                _ => false,
            };
            if ambiguous {
                return;
            }
        }
        // factory_call: (send #factory_call? %FACTORY_CALLS {sym str send lvar} _*)
        if !helpers::factory_bot::factory_call(cx, call.receiver().as_ref()) {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        let first_ok = helpers::rspec::is_sym(cx, first)
            || helpers::rspec::is_str(first)
            || helpers::rspec::send_call(first).is_some_and(|c| !is_csend(&c))
            || helpers::node_types::lvar_name(first).is_some();
        if !first_ok {
            return;
        }
        if name == "generate" && args.len() > 1 {
            return;
        }
        let parenthesized = call.opening_loc().is_some_and(|o| o.as_slice() == b"(");
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let style = cx.cfg.str("EnforcedStyle");
        if style != Some("require_parentheses") && parenthesized {
            let same_line = cx.src.line_of(node.location().start_offset()) == cx.src.line_of(first.location().start_offset());
            // omit_hash_value?: a last hash with a `key:` pair
            let omitted = args.len() > 1
                && args.last().and_then(helpers::rails::hash_elements).is_some_and(|ps| {
                    ps.iter().filter_map(|p| p.as_assoc_node()).any(|p| p.value().as_implicit_node().is_some())
                });
            if same_line && !omitted {
                let o = call.opening_loc().unwrap();
                let c = call.closing_loc();
                let (os, oe) = (o.start_offset(), o.end_offset());
                cx.add_fix(ss, se, "Prefer method call without parentheses", |cr| {
                    cr.replace(os, oe, " ");
                    if let Some(c) = c {
                        cr.remove(c.start_offset(), c.end_offset());
                    }
                });
            }
        }
        if style != Some("omit_parentheses") && !parenthesized {
            let Some(fix) = helpers::add_parentheses::plan(node, cx.src.text) else { return };
            cx.add_fix(ss, se, "Prefer method call with parentheses", |c| helpers::add_parentheses::apply(c, &fix));
        }
    }
}
