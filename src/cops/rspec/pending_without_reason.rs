use crate::prelude::*;

pub const NAME: &str = "RSpec/PendingWithoutReason";
pub static COP: PendingWithoutReason = PendingWithoutReason;
pub struct PendingWithoutReason;

impl Cop for PendingWithoutReason {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let l = helpers::rspec::lang(cx);
        let n = method_name(&call);
        let has = |list: &helpers::rspec::List| list.has(n);
        let args = send_args(&call);
        let (s, e) = send_range(&call);
        // metadata_without_reason?: the last argument is :pending/:skip or a hash with `pending: true`/`skip: true`
        // (NodePattern checks the arity, an argument, before it calls the predicates.)
        if !args.is_empty() && helpers::rspec::rspec_recv(&call) && (l.example_group(n) || l.example(n)) {
            let key = |a: &Node| {
                helpers::rspec::is_sym(cx, a)
                    .then(|| helpers::rails::sym_value(a))
                    .flatten()
                    .filter(|v| v == "pending" || v == "skip")
            };
            let found = args.last().and_then(|last| {
                key(last).or_else(|| {
                    helpers::rails::hash_elements(last)?
                        .iter()
                        .filter_map(|p| p.as_assoc_node())
                        .find_map(|p| key(&p.key()).filter(|_| p.value().as_true_node().is_some()))
                })
            });
            if let Some(pending) = found {
                cx.add(s, e, format!("Give the reason for {pending}."));
            }
        }
        // parent_node: the parser parent of the node (or of its block), past a `begin`
        let anc = cx.ancestors;
        // parser parent of the node, or of its block (the same Prism node)
        let Some(mut pi) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return };
        if anc[pi].as_statements_node().is_some_and(|st| st.body().iter().count() > 1)
            && let Some(p) = helpers::parser_parent::parser_parent_index(anc, pi)
        {
            pi = p;
        }
        let parent = &anc[pi];
        // A BlockNode parent stands for its call's block.
        let parent_owner = if parent.as_block_node().is_some() && pi > 0 { &anc[pi - 1] } else { parent };
        let block = helpers::rspec::block_of(&call);
        // block_node_example_group?, asked only when the parent is no spec group
        let block_group = || {
            block.is_some()
                && helpers::rspec::example_group(&l, node)
                && call.receiver().is_some_and(|r| helpers::rspec::is_explicit_rspec(&r))
        };
        let parent_is_block = parent.as_block_node().is_some();
        if (parent_is_block && helpers::rspec::spec_group(&l, parent_owner)) || block_group() {
            // skipped_by_example_method?: (send nil? ${#Examples.skipped #Examples.pending})
            if call.receiver().is_none() && args.is_empty() && (has(&l.ex_skipped) || has(&l.ex_pending)) {
                cx.add(s, e, format!("Give the reason for {n}."));
            }
            // skipped_by_example_method_with_block?(node.parent): `(any_block (send nil?
            // ${#Examples.skipped #Examples.pending} ...) ...)`, the node's own block, or the block
            // whose body the node is
            let parent_block = if block.is_some() {
                node.as_call_node()
            } else {
                helpers::parser_parent::parser_parent_index(anc, anc.len())
                    .filter(|&i| i > 0 && anc[i].as_block_node().is_some())
                    .and_then(|i| anc[i - 1].as_call_node())
            };
            if let Some(pc) = parent_block.filter(|pc| pc.receiver().is_none()) {
                let pn = method_name(&pc);
                if l.ex_skipped.has(pn) || l.ex_pending.has(pn) {
                    cx.add(s, e, format!("Give the reason for {pn}."));
                }
            }
            // skipped_by_example_group_method?: (send #rspec? ${#ExampleGroups.skipped} ...)
            if helpers::rspec::rspec_recv(&call) && has(&l.eg_skipped) {
                cx.add(s, e, "Give the reason for skip.");
            }
        } else if parent_is_block && helpers::rspec::example(&l, parent_owner) {
            // skipped_in_example?: (send nil? ${#Examples.skipped #Examples.pending})
            if call.receiver().is_none() && args.is_empty() && (has(&l.ex_skipped) || has(&l.ex_pending)) {
                cx.add(s, e, format!("Give the reason for {n}."));
            }
        }
    }
}
