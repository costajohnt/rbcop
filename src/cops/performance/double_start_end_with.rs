use crate::prelude::*;

pub const NAME: &str = "Performance/DoubleStartEndWith";
pub static COP: DoubleStartEndWith = DoubleStartEndWith;
pub struct DoubleStartEndWith;

impl Cop for DoubleStartEndWith {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::OR_NODE, kind::AND_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (left, right, negated) = match node.as_or_node() {
            Some(or) => (or.left(), or.right(), false),
            None => {
                let and = node.as_and_node().unwrap();
                (and.left(), and.right(), true)
            }
        };
        let (Some((bang, first)), Some((_, second))) = (unwrap(left, negated), unwrap(right, negated)) else { return };
        // (call $_recv [%methods_to_check $_method] $...) and (call _recv _method $...)
        if [&first, &second].iter().any(|c| c.block().is_some_and(|b| b.as_block_node().is_some())) {
            return;
        }
        let method = method_name(&first);
        let methods: &[&str] = if cx.cfg.bool("IncludeActiveSupportAliases") == Some(true) {
            &["start_with?", "starts_with?", "end_with?", "ends_with?"]
        } else {
            &["start_with?", "end_with?"]
        };
        if !methods.contains(&method) || method_name(&second) != method {
            return;
        }
        let (Some(recv), Some(recv2)) = (first.receiver(), second.receiver()) else { return };
        let span = |n: &Node| (n.location().start_offset(), n.location().end_offset());
        if !helpers::node_equality::same(cx.src, span(&recv), span(&recv2)) {
            return;
        }
        let first_args = send_args(&first);
        let second_args = send_args(&second);
        if !second_args.iter().all(helpers::pure::pure) {
            return;
        }
        // The corrector reads the first call's first and the second call's last argument.
        let (Some(a), Some(b)) = (first_args.first(), second_args.last()) else { return };
        let combined: Vec<&str> = first_args.iter().chain(second_args.iter()).map(|a| node_text(a)).collect();
        let combined = combined.join(", ");
        let dot = if is_csend(&first) { "&." } else { "." };
        let bang = if bang == Some(true) { "!" } else { "" };
        let loc = node.location();
        let msg = format!(
            "Use `{bang}{}{dot}{method}({combined})` instead of `{}`.",
            node_text(&recv),
            cx.src.slice(loc.start_offset(), loc.end_offset())
        );
        let (start, end) = (a.location().start_offset(), b.location().end_offset());
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| c.replace(start, end, &combined));
    }
}

/// An operand: the call itself, or under on_and the call a `!` negates (with whether it is
/// spelled `!`).
fn unwrap<'pr>(n: Node<'pr>, negated: bool) -> Option<(Option<bool>, ruby_prism::CallNode<'pr>)> {
    let c = n.as_call_node()?;
    if !negated {
        return Some((None, c));
    }
    if is_csend(&c) || method_name(&c) != "!" || !send_args(&c).is_empty() || c.block().is_some() {
        return None;
    }
    let bang = c.message_loc().is_some_and(|m| m.as_slice() == b"!");
    Some((Some(bang), c.receiver()?.as_call_node()?))
}
