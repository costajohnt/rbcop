use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/HashFetchChain";
pub static COP: HashFetchChain = HashFetchChain;
pub struct HashFetchChain;

/// `(call _ :fetch $_arg {nil (hash) (send (const {nil? cbase} :Hash) :new)})`
/// `first`: the visited send itself (a receiver with a block is a parser `block`, not a call).
fn diggable<'pr>(node: &Node<'pr>, first: bool) -> Option<(CallNode<'pr>, Node<'pr>)> {
    let call = node.as_call_node()?;
    if method_name(&call) != "fetch" || (!first && call.block().is_some_and(|b| b.as_block_node().is_some())) {
        return None;
    }
    let mut args = send_args(&call);
    if args.len() != 2 {
        return None;
    }
    let default = args.pop().unwrap();
    let ok = default.as_nil_node().is_some()
        || default.as_hash_node().is_some_and(|h| h.elements().iter().next().is_none())
        || default.as_call_node().is_some_and(|c| {
            !is_csend(&c)
                && method_name(&c) == "new"
                && c.arguments().is_none()
                && c.block().is_none()
                && c.receiver().is_some_and(|r| is_top_const(&r, "Hash"))
        });
    ok.then(|| (call, args.pop().unwrap()))
}

impl Cop for HashFetchChain {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "fetch" {
            return;
        }
        let range = send_range(&call);
        if cx.state::<Vec<(usize, usize)>>().contains(&range) {
            return;
        }
        // last_fetch_non_nil?
        if !send_args(&call).last().is_some_and(|a| a.as_nil_node().is_some()) {
            return;
        }
        let mut arguments = Vec::new();
        let mut last_selector = None;
        let mut current = Some(call.as_node());
        let mut first = true;
        while let Some((c, arg)) = current.as_ref().and_then(|n| diggable(n, first)) {
            first = false;
            arguments.insert(0, node_text(&arg));
            cx.state::<Vec<(usize, usize)>>().push(send_range(&c));
            last_selector = c.message_loc().map(|l| l.start_offset());
            current = c.receiver();
        }
        let Some(sel) = last_selector else { return };
        if arguments.len() < 2 {
            return;
        }
        // `selector.join(node.loc.end)`: RuboCop raises without parentheses.
        let Some(end) = call.closing_loc().map(|l| l.end_offset()) else { return };
        let replacement = format!("dig({})", arguments.join(", "));
        let msg = format!("Use `{replacement}` instead.");
        cx.add_fix(sel, end, msg, |c| c.replace(sel, end, &replacement));
    }
}
