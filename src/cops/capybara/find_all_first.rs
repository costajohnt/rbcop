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
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "all" | "find") {
            return;
        }
        let args = send_args(&call);
        let Some(sel) = call.message_loc() else { return };
        let selector = args.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", ");
        // on_all_first: `all(...).first` or `all(...)[0]`, outside `and` / `or`
        if name == "all"
            && !args.is_empty()
            && helpers::rspec::block_of(&call).is_none()
            && let Some(parent) = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node())
            && !is_csend(&parent)
            && parent.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset() && r.as_call_node().is_some())
            && match (method_name(&parent), &send_args(&parent)[..]) {
                ("first", []) => true,
                ("[]", [i]) => i.as_integer_node().is_some() && int_literal_value(i) == Some(0),
                _ => false,
            }
            && !cx.ancestors.iter().any(helpers::parser_parent::is_operator_keyword)
            && let Some(psel) = parent.message_loc()
        {
            let (s, e) = (sel.start_offset(), psel.end_offset());
            let replacement = format!("first({selector})");
            cx.add_fix(s, e, format!("Use `first({selector})`."), |c| c.replace(s, e, &replacement));
        }
        // on_match_first: `find(x, match: :first)`
        let [first, hash] = &args[..] else { return };
        let Some(pairs) = helpers::rails::hash_elements(hash) else { return };
        let sym = |n: &Node, v: &str| helpers::rspec::is_sym(cx, n) && helpers::rails::sym_value(n).as_deref() == Some(v);
        if !pairs.iter().filter_map(|p| p.as_assoc_node()).any(|p| sym(&p.key(), "match") && sym(&p.value(), "first")) {
            return;
        }
        let mut parts = vec![node_text(first)];
        parts.extend(pairs.iter().map(|p| node_text(p)).filter(|s| *s != "match: :first"));
        let selector = parts.join(", ");
        let (s, e) = (sel.start_offset(), send_range(&call).1);
        let replacement = format!("first({selector})");
        cx.add_fix(s, e, format!("Use `first({selector})`."), |c| c.replace(s, e, &replacement));
    }
}
