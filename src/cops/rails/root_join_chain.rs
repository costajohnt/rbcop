use crate::prelude::*;

pub const NAME: &str = "Rails/RootJoinChain";
pub static COP: RootJoinChain = RootJoinChain;
pub struct RootJoinChain;

/// (send (const {nil? cbase} :Rails) {:root :public_path})
fn rails_root(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| {
        matches!(method_name(&c), "root" | "public_path")
            && send_args(&c).is_empty()
            && c.receiver().is_some_and(|r| helpers::rails::is_const(&r, "Rails"))
    })
}

/// (send _ :join $...)
fn join<'pr>(n: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    helpers::rails::send_node(n).filter(|c| method_name(c) == "join")
}

impl Cop for RootJoinChain {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "join" {
            return;
        }
        let anc = cx.ancestors;
        if helpers::rails::parent_index(anc).is_some_and(|i| join(&anc[i]).is_some()) {
            return;
        }
        if call.receiver().is_some_and(|r| rails_root(&r)) {
            return;
        }
        // The node itself is a send even with a block (parser `on_send` sees the inner send).
        let mut args: Vec<String> = Vec::new();
        let mut cur: Node = call.as_node();
        let mut first = true;
        loop {
            let c = if first { Some(call.as_node().as_call_node().unwrap()) } else { join(&cur) };
            first = false;
            let Some(c) = c else { break };
            let mut a: Vec<String> = send_args(&c).iter().map(|x| node_text(x).to_string()).collect();
            a.extend(args);
            args = a;
            let Some(r) = c.receiver() else { return };
            cur = r;
        }
        if !rails_root(&cur) {
            return;
        }
        let rails = cur.as_call_node().unwrap();
        let sel_end = rails.message_loc().unwrap().end_offset();
        let (s, e) = send_range(&call);
        let msg = format!("Use `{}.join(...)` instead of chaining `#join` calls.", node_text(&cur));
        let replacement = format!(".join({})", args.join(", "));
        cx.add_fix(s, e, msg, |c| c.replace(sel_end, e, &replacement));
    }
}
