use crate::prelude::*;

pub const NAME: &str = "RSpecRails/ReceivePerformLater";
pub static COP: ReceivePerformLater = ReceivePerformLater;
pub struct ReceivePerformLater;

impl Cop for ReceivePerformLater {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? {:receive :have_received} (sym :perform_later))
        let call = node.as_call_node().unwrap();
        let matcher = method_name(&call);
        // a matcher with a block is a `block` node: never the runner's matcher send
        if call.receiver().is_some() || !matches!(matcher, "receive" | "have_received") || call.block().is_some() {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        if !helpers::rspec::is_sym(cx, arg) || helpers::rails::sym_value(arg).as_deref() != Some("perform_later") {
            return;
        }
        // find_runner_node: the nearest `send` ancestor named to/to_not/not_to
        let anc = cx.ancestors;
        let runner = (0..anc.len()).rev().find_map(|i| {
            let c = anc[i].as_call_node()?;
            let owns_block = anc.get(i + 1).is_some_and(|n| n.as_block_node().is_some());
            (!owns_block && !is_csend(&c) && helpers::rspec::is_runner(method_name(&c))).then_some(c)
        });
        let Some(runner) = runner else { return };
        // runner_matcher?: the runner's first argument, or a receiver down its chain, is this node
        let me = helpers::rspec::node_range(node);
        let mut cur = send_args(&runner).into_iter().next();
        while let Some(n) = cur.as_ref().and_then(|n| helpers::rspec::send_call(n)).filter(|c| !is_csend(c)) {
            if helpers::rspec::node_range(&n.as_node()) == me {
                break;
            }
            cur = n.receiver();
        }
        if cur.is_none_or(|n| helpers::rspec::node_range(&n) != me) {
            return;
        }
        // expect_or_allow?: (send nil? {:expect :allow} {const_type? (send nil? :described_class)})
        let Some(expect) = runner.receiver().and_then(|r| helpers::rspec::send_call(&r)) else { return };
        let receiver = method_name(&expect);
        if expect.receiver().is_some() || !matches!(receiver, "expect" | "allow") {
            return;
        }
        let [job] = &send_args(&expect)[..] else { return };
        let job_ok = matches!(kind::of(job), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE)
            || helpers::rspec::bare_send(job, "described_class").is_some();
        if !job_ok || (receiver == "allow" && matcher == "receive") {
            return;
        }
        let to = method_name(&runner);
        let job_class = node_text(job);
        let (s, e) = send_range(&runner);
        cx.add(
            s,
            e,
            format!(
                "Prefer `expect {{ ... }}.{to} have_enqueued_job({job_class})` over `{receiver}({job_class}).{to} {matcher}(:perform_later)`."
            ),
        );
    }
}
