use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Performance/ChainArrayAllocation";
pub static COP: ChainArrayAllocation = ChainArrayAllocation;
pub struct ChainArrayAllocation;

const RETURN_NEW_ARRAY_WHEN_ARGS: &[&str] = &["first", "last", "pop", "sample", "shift"];
const RETURNS_NEW_ARRAY_WHEN_NO_BLOCK: &[&str] = &["zip", "product"];
#[rustfmt::skip]
const ALWAYS_RETURNS_NEW_ARRAY: &[&str] = &[
    "*", "+", "-", "collect", "collect_concat", "compact", "drop", "drop_while", "filter_map", "flat_map", "flatten",
    "map", "reject", "reverse", "rotate", "select", "shuffle", "sort", "take", "take_while", "transpose", "uniq",
    "values_at", "|",
];
#[rustfmt::skip]
const HAS_MUTATION_ALTERNATIVE: &[&str] =
    &["collect", "compact", "flatten", "map", "reject", "reverse", "rotate", "select", "shuffle", "sort", "uniq"];

impl Cop for ChainArrayAllocation {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let second = method_name(&call);
        if is_csend(&call) || !HAS_MUTATION_ALTERNATIVE.contains(&second) {
            return;
        }
        let Some(recv) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if is_csend(&recv) {
            return;
        }
        let first = method_name(&recv);
        let recv_args = send_args(&recv);
        let block = recv.block().filter(|b| b.as_block_node().is_some());
        let matched = if block.is_some() {
            // (any_block (send _ $%ALWAYS_RETURNS_NEW_ARRAY) ...)
            ALWAYS_RETURNS_NEW_ARRAY.contains(&first) && recv_args.is_empty()
        } else if RETURN_NEW_ARRAY_WHEN_ARGS.contains(&first) {
            // (send _ $%RETURN_NEW_ARRAY_WHEN_ARGS {int lvar ivar cvar gvar send})
            matches!(recv_args.as_slice(), [a] if simple_arg(a))
        } else {
            // (send _ $%RETURNS_NEW_ARRAY ...)
            ALWAYS_RETURNS_NEW_ARRAY.contains(&first) || RETURNS_NEW_ARRAY_WHEN_NO_BLOCK.contains(&first)
        };
        if !matched {
            return;
        }
        // node.each_descendant(:send).any? { |d| d.method?(:lazy) }
        let mut lazy = Lazy(false);
        lazy.visit(&recv.as_node());
        if let Some(a) = call.arguments() {
            lazy.visit_arguments_node(&a);
        }
        if let Some(b) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
            lazy.visit(&b);
        }
        if lazy.0 {
            return;
        }
        // enumerable_select_method?: `select` needs a block receiver whose send has no arguments.
        if second == "select" && block.is_none() {
            return;
        }
        let start = call.message_loc().unwrap().start_offset();
        let end = send_range(&call).1;
        cx.add(
            start,
            end,
            format!(
                "Use unchained `{first}` and `{second}!` (followed by `return array` if required) instead of chaining `{first}...{second}`."
            ),
        );
    }
}

/// `{int lvar ivar cvar gvar send}`
fn simple_arg(node: &Node) -> bool {
    match kind::of(node) {
        kind::INTEGER_NODE
        | kind::LOCAL_VARIABLE_READ_NODE
        | kind::IT_LOCAL_VARIABLE_READ_NODE
        | kind::INSTANCE_VARIABLE_READ_NODE
        | kind::CLASS_VARIABLE_READ_NODE
        | kind::GLOBAL_VARIABLE_READ_NODE => true,
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            !is_csend(&c) && !c.block().is_some_and(|b| b.as_block_node().is_some())
        }
        _ => false,
    }
}

/// A `send` (not csend) named `lazy` anywhere below.
struct Lazy(bool);

impl<'pr> Visit<'pr> for Lazy {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.0 |= !is_csend(node) && method_name(node) == "lazy";
        ruby_prism::visit_call_node(self, node);
    }
}
