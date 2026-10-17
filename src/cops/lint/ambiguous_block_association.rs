use crate::prelude::*;

pub const NAME: &str = "Lint/AmbiguousBlockAssociation";
pub static COP: AmbiguousBlockAssociation = AmbiguousBlockAssociation;
pub struct AmbiguousBlockAssociation;

const BLOCK_METHODS: &[&str] = &[
    "map", "collect", "flat_map", "collect_concat", "select", "filter", "find_all", "reject", "find", "detect", "each",
    "each_with_object", "each_with_index", "reduce", "inject", "sort_by", "min_by", "max_by", "group_by", "filter_map",
];

impl Cop for AmbiguousBlockAssociation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        on_send(cx, &call);
        if call.block().is_some_and(|b| b.as_block_node().is_some()) {
            on_block(cx, &call);
        }
    }
}

/// A call with a literal block (parser `block`), and its block.
fn block_of<'pr>(node: &Node<'pr>) -> Option<(ruby_prism::CallNode<'pr>, ruby_prism::BlockNode<'pr>)> {
    let call = node.as_call_node()?;
    let block = call.block()?.as_block_node()?;
    Some((call, block))
}

fn on_send(cx: &mut Cx, call: &ruby_prism::CallNode) {
    // A `&blk` argument is parser's last argument.
    if call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
        return;
    }
    let args = call_args(call);
    let Some(last) = args.last() else { return };
    // ambiguous_block_association?: the last argument is a block whose send has no arguments.
    let ambiguous = match last {
        Node::LambdaNode { .. } => true,
        _ => block_of(last).is_some_and(|(c, _)| send_args(&c).is_empty()),
    };
    if !ambiguous || call.opening_loc().is_some() || helpers::node_types::lambda_or_proc(last) {
        return;
    }
    let name = method_name(call);
    let last_name = match last {
        Node::LambdaNode { .. } => "lambda",
        _ => method_name(&last.as_call_node().unwrap()),
    };
    let (send_start, send_end) = match last {
        Node::LambdaNode { .. } => {
            let o = last.as_lambda_node().unwrap().operator_loc();
            (o.start_offset(), o.end_offset())
        }
        _ => send_range(&last.as_call_node().unwrap()),
    };
    let send_src = &cx.src.text[send_start..send_end];
    if call.equal_loc().is_some()
        || helpers::node_types::is_operator_method(name)
        || name == "[]"
        || helpers::allowed::allowed_method(cx.cfg, last_name)
        || helpers::allowed::matches_allowed_pattern(cx.cfg, send_src)
    {
        return;
    }
    let msg = format!(
        "Parenthesize the param `{}` to make sure that the block will be associated with the `{send_src}` method call.",
        node_text(last)
    );
    let (start, end) = send_range(call);
    let Some(selector) = call.message_loc() else { return }; // RuboCop crashes on `loc.selector.end`
    let first_start = args[0].location().start_offset();
    let last_end = last.location().end_offset();
    let sel_end = selector.end_offset();
    cx.add_fix(start, end, msg, |c| {
        c.remove(sel_end, first_start);
        c.insert_before(sel_end, first_start, "(");
        c.insert_after(last.location().start_offset(), last_end, ")");
    });
}

fn on_block(cx: &mut Cx, call: &ruby_prism::CallNode) {
    let block = call.block().unwrap().as_block_node().unwrap();
    if block.opening_loc().as_slice() == b"{" || is_csend(call) || call.opening_loc().is_some() {
        return;
    }
    let args = call_args(call);
    let Some(last) = args.last() else { return };
    let (_, send_end) = send_range(call);
    // last_argument.each_node(:call).find: preorder, the first call ending where the send ends.
    let mut found = None;
    find_call(last, send_end, &mut found);
    let Some((start, end, inner)) = found else { return };
    if helpers::allowed::allowed_method(cx.cfg, &inner)
        || helpers::allowed::matches_allowed_pattern(cx.cfg, &cx.src.text[start..end])
    {
        return;
    }
    let msg = format!(
        "`{inner}` is called without a block because the `do` block binds to `{}`. Use braces or extract to a variable.",
        method_name(call)
    );
    cx.add_with(start, end, msg, false);
}

/// Preorder search for a send/csend (parser ranges) ending at `end` with a BLOCK_METHODS name and
/// no arguments.
fn find_call(node: &Node, end: usize, found: &mut Option<(usize, usize, String)>) {
    use ruby_prism::Visit;
    struct V<'a> {
        end: usize,
        found: &'a mut Option<(usize, usize, String)>,
    }
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            if self.found.is_some() {
                return;
            }
            // A call with a literal block is parser's block node wrapping the send.
            let (s, e) = send_range(node);
            let name = method_name(node);
            if e == self.end && BLOCK_METHODS.contains(&name) && send_args(node).is_empty() {
                *self.found = Some((s, e, name.to_string()));
                return;
            }
            ruby_prism::visit_call_node(self, node);
        }
    }
    V { end, found }.visit(node);
}
