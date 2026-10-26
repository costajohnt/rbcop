use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Capybara/RSpec/FindElementValueEq";
pub static COP: FindElementValueEq = FindElementValueEq;
pub struct FindElementValueEq;

/// A parser `send`: not `&.`, no literal block.
fn send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && helpers::rspec::block_of(c).is_none())
}

/// replacement_value: None for a splat or a hash with a double splat.
fn replacement_value(n: &Node) -> Option<String> {
    let elements = helpers::rails::hash_elements(n);
    if n.as_splat_node().is_some() || elements.as_ref().is_some_and(|e| e.iter().any(|x| x.as_assoc_splat_node().is_some())) {
        return None;
    }
    let src = node_text(n);
    if n.as_keyword_hash_node().is_some() {
        return Some(format!("{{{src}}}"));
    }
    // method_call_with_no_parentheses?
    if send(n).is_some_and(|c| !send_args(&c).is_empty() && c.opening_loc().is_none()) {
        return Some(format!("({src})"));
    }
    Some(src.to_string())
}

impl Cop for FindElementValueEq {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send nil? :expect (send $_ :value)) ${:to :to_not :not_to} $(send nil? :eq $_)), on the parent
        let Some(call) = send(node).filter(|c| c.receiver().is_none() && method_name(c) == "expect") else { return };
        let [value] = &send_args(&call)[..] else { return };
        let Some(value) = send(value).filter(|v| method_name(v) == "value" && send_args(v).is_empty()) else { return };
        // the runner's own block (`.to eq(x) do ... end`) does not make it a parser `block` here
        let Some(runner) = helpers::parser_parent::parser_parent(cx.ancestors).and_then(|p| p.as_call_node()) else { return };
        let to = method_name(&runner);
        if is_csend(&runner)
            || !matches!(to, "to" | "to_not" | "not_to")
            || !runner.receiver().is_some_and(|r| r.location().start_offset() == node.location().start_offset())
        {
            return;
        }
        let [matcher] = &send_args(&runner)[..] else { return };
        let Some(m) = send(matcher).filter(|m| m.receiver().is_none() && method_name(m) == "eq") else { return };
        let [expected] = &send_args(&m)[..] else { return };
        // replacement_for_finder
        let Some(finder) = value.receiver().and_then(|f| send(&f)) else { return };
        let fargs = send_args(&finder);
        let target = finder.receiver().map_or("page".to_string(), |r| node_text(&r).to_string());
        let sym = |n: &Node, v: &str| helpers::rspec::is_sym(cx, n) && helpers::rails::sym_value(n).as_deref() == Some(v);
        let field_name = |n: &Node| {
            helpers::rspec::is_str(n) && matches!(n.as_string_node().unwrap().unescaped(), b"input" | b"textarea")
        };
        let locator: Option<&Node> = match method_name(&finder) {
            "find" => match &fargs[..] {
                [t, l] if sym(t, "field") => Some(l),
                [n] if field_name(n) => None,
                [t, n] if sym(t, "css") && field_name(n) => None,
                _ => return,
            },
            "find_field" => match &fargs[..] {
                [l] => Some(l),
                _ => return,
            },
            _ => return,
        };
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let msg = "Prefer `have_field` over checking `find(...).value`.";
        // replacement_matcher
        let Some(with) = replacement_value(expected) else { return cx.add_with(s, e, msg, false) };
        let mut args = Vec::new();
        if let Some(l) = locator {
            args.push(if l.as_keyword_hash_node().is_some() { node_text(l).to_string() } else { replacement_value(l).unwrap_or_default() });
        }
        args.push(format!("with: {with}"));
        let name = if to == "to" { "have_field" } else { "have_no_field" };
        let replacement = format!("{name}({})", args.join(", "));
        let (vs, ve) = helpers::rspec::node_range(&value.as_node());
        let rsel = runner.message_loc().unwrap();
        let (ms, me) = helpers::rspec::node_range(matcher);
        cx.add_fix(s, e, msg, |c| {
            c.replace(vs, ve, &target);
            c.replace(rsel.start_offset(), rsel.end_offset(), "to");
            c.replace(ms, me, &replacement);
        });
    }
}
