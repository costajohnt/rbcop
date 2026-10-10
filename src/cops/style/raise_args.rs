use crate::helpers::const_name::const_name;
use crate::helpers::parser_parent::{is_operator_keyword, is_ternary, parser_parent};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/RaiseArgs";
pub static COP: RaiseArgs = RaiseArgs;
pub struct RaiseArgs;

impl Cop for RaiseArgs {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if call.receiver().is_some() || !matches!(method, "raise" | "fail") {
            return;
        }
        let args = send_args(&call);
        let parens = parser_parent(cx.ancestors).is_some_and(|p| is_operator_keyword(p) || is_ternary(p));
        let (s, e) = send_range(&call);
        let wrap = |arguments: String| {
            if parens { format!("{method}({arguments})") } else { format!("{method} {arguments}") }
        };
        if cx.cfg.str("EnforcedStyle") == Some("compact") {
            if args.len() <= 1 {
                return;
            }
            let exception = &args[0];
            if plain_send(exception).is_some_and(|c| call_args(&c).first().is_some_and(is_hash)) {
                return;
            }
            let fix = (args.len() == 2).then(|| {
                let class = block_or_send_receiver(exception).map_or(node_text(exception), |r| node_text(&r));
                wrap(format!("{class}.new({})", node_text(&args[1])))
            });
            let msg = format!("Provide an exception object as an argument to `{method}`.");
            return cx.add_fix(s, e, msg, |c| {
                if let Some(fix) = &fix {
                    c.replace(s, e, fix);
                }
            });
        }
        let [first] = args.as_slice() else { return };
        let Some(new) = plain_send(first) else { return };
        let Some(recv) = new.receiver().filter(|_| method_name(&new) == "new") else { return };
        let new_args = send_args(&new);
        let acceptable = match new_args.as_slice() {
            [] => false,
            [arg] => matches!(
                kind::of(arg),
                kind::HASH_NODE | kind::KEYWORD_HASH_NODE | kind::SPLAT_NODE | kind::FORWARDING_ARGUMENTS_NODE
            ),
            _ => true,
        };
        if acceptable {
            return;
        }
        if const_name(&recv).is_some_and(|n| cx.cfg.strings("AllowedCompactTypes").contains(&n)) {
            return;
        }
        let arguments = std::iter::once(node_text(&recv)).chain(new_args.first().map(node_text)).collect::<Vec<_>>();
        let fix = wrap(arguments.join(", "));
        let msg = format!("Provide an exception class and message as arguments to `{method}`.");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &fix));
    }
}

/// A parser `send` (not csend, no attached block).
fn plain_send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && !c.block().is_some_and(|b| b.as_block_node().is_some()))
}

fn is_hash(node: &Node) -> bool {
    matches!(kind::of(node), kind::HASH_NODE | kind::KEYWORD_HASH_NODE)
}

/// rubocop-ast `receiver`: `(send $_ ...)` or `(block (call $_ ...) ...)`.
fn block_or_send_receiver<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = node.as_call_node()?;
    let block = call.block().is_some_and(|b| b.as_block_node().is_some());
    if !block && is_csend(&call) {
        return None;
    }
    call.receiver()
}
