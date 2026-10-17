use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Performance/CollectionLiteralInLoop";
pub static COP: CollectionLiteralInLoop = CollectionLiteralInLoop;
pub struct CollectionLiteralInLoop;

/// `Enumerable.instance_methods + [:each]` on Ruby 4.0.6.
#[rustfmt::skip]
const ENUMERABLE: &[&str] = &[
    "all?", "any?", "chain", "chunk", "chunk_while", "collect", "collect_concat", "compact", "count", "cycle",
    "detect", "drop", "drop_while", "each", "each_cons", "each_entry", "each_slice", "each_with_index",
    "each_with_object", "entries", "filter", "filter_map", "find", "find_all", "find_index", "first", "flat_map",
    "grep", "grep_v", "group_by", "include?", "inject", "lazy", "map", "max", "max_by", "member?", "min", "min_by",
    "minmax", "minmax_by", "none?", "one?", "partition", "reduce", "reject", "reverse_each", "select",
    "slice_after", "slice_before", "slice_when", "sort", "sort_by", "sum", "take", "take_while", "tally", "to_a",
    "to_h", "to_set", "uniq", "zip",
];
#[rustfmt::skip]
const NONMUTATING_ARRAY_METHODS: &[&str] = &[
    "&", "*", "+", "-", "<=>", "==", "[]", "all?", "any?", "assoc", "at", "bsearch", "bsearch_index", "collect",
    "combination", "compact", "count", "cycle", "deconstruct", "difference", "dig", "drop", "drop_while", "each",
    "each_index", "empty?", "eql?", "fetch", "filter", "find_index", "first", "flatten", "hash", "include?",
    "index", "inspect", "intersection", "join", "last", "length", "map", "max", "min", "minmax", "none?", "one?",
    "pack", "permutation", "product", "rassoc", "reject", "repeated_combination", "repeated_permutation",
    "reverse", "reverse_each", "rindex", "rotate", "sample", "select", "shuffle", "size", "slice", "sort", "sum",
    "take", "take_while", "to_a", "to_ary", "to_h", "to_s", "transpose", "union", "uniq", "values_at", "zip", "|",
];
#[rustfmt::skip]
const NONMUTATING_HASH_METHODS: &[&str] = &[
    "<", "<=", "==", ">", ">=", "[]", "any?", "assoc", "compact", "dig", "each", "each_key", "each_pair",
    "each_value", "empty?", "eql?", "fetch", "fetch_values", "filter", "flatten", "has_key?", "has_value?", "hash",
    "include?", "inspect", "invert", "key", "key?", "keys?", "length", "member?", "merge", "rassoc", "rehash",
    "reject", "select", "size", "slice", "to_a", "to_h", "to_hash", "to_proc", "to_s", "transform_keys",
    "transform_values", "value?", "values", "values_at",
];

impl Cop for CollectionLiteralInLoop {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let method = method_name(&call);
        let Some(recv) = call.receiver() else { return };
        let (class, size) = match &recv {
            Node::ArrayNode { .. } if ENUMERABLE.contains(&method) || NONMUTATING_ARRAY_METHODS.contains(&method) => {
                ("Array", recv.as_array_node().unwrap().elements().iter().count())
            }
            Node::HashNode { .. } if ENUMERABLE.contains(&method) || NONMUTATING_HASH_METHODS.contains(&method) => {
                ("Hash", recv.as_hash_node().unwrap().elements().iter().count())
            }
            _ => return,
        };
        let min_size = match cx.cfg.get("MinSize") {
            None | Some(Value::Null | Value::Bool(false)) => 1,
            Some(Value::Int(i)) => *i as usize,
            Some(v) => match v.as_string().and_then(|s| s.trim().parse::<i64>().ok()) {
                Some(i) => i as usize,
                // Integer() raises: the cop errors, no offense.
                None => return,
            },
        };
        if size < min_size || !helpers::literal_node::recursive_basic_literal(&recv) {
            return;
        }
        if class == "Array" && method == "include?" && cx.config.target_ruby >= 3.4 && optimized_include(&call) {
            return;
        }
        // parent_is_loop?: any parser ancestor of the literal is a loop.
        let recv_span = (recv.location().start_offset(), recv.location().end_offset());
        let anc = cx.ancestors;
        let owners = anc.iter().filter_map(|a| a.as_call_node()).chain(std::iter::once(call));
        let mut in_loop = anc.iter().any(|a| matches!(kind::of(a), kind::WHILE_NODE | kind::UNTIL_NODE | kind::FOR_NODE));
        for owner in owners {
            if in_loop {
                break;
            }
            // A call owning a plain block is that block, an ancestor of everything inside.
            let Some(b) = owner.block().and_then(|b| b.as_block_node()) else { continue };
            if b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some()) {
                continue;
            }
            if is_csend(&owner) {
                continue;
            }
            let name = method_name(&owner);
            let owner_recv = owner.receiver();
            // kernel_loop?: (block (send {nil? (const nil? :Kernel)} :loop) ...)
            if name == "loop"
                && send_args(&owner).is_empty()
                && owner_recv.as_ref().is_none_or(|r| r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Kernel"))
            {
                in_loop = true;
                continue;
            }
            // enumerable_loop? with the literal neither its receiver nor inside it (structurally).
            if ENUMERABLE.contains(&name) {
                let excluded = owner_recv.as_ref().is_some_and(|r| contains_equal(cx.src, r, recv_span, kind::of(&recv)));
                in_loop = !excluded;
            }
        }
        if !in_loop {
            return;
        }
        cx.add(
            recv_span.0,
            recv_span.1,
            format!("Avoid immutable {class} literals in loops. It is better to extract it into a local variable or a constant."),
        );
    }
}


/// optimized_array_include?: `include?` of one argument that is (or whose argument-less send
/// chain ends at) a str, self, lvar, ivar or receiver-less send.
fn optimized_include(call: &ruby_prism::CallNode) -> bool {
    let args = send_args(call);
    let [arg] = args.as_slice() else { return false };
    optimized_operand(arg)
}

fn optimized_operand(arg: &Node) -> bool {
    let send = arg.as_call_node().filter(|c| !is_csend(c) && !c.block().is_some_and(|b| b.as_block_node().is_some()));
    match send {
        Some(c) if !send_args(&c).is_empty() => false,
        Some(c) => c.receiver().is_none_or(|r| optimized_operand(&r)),
        None => {
            (arg.as_string_node().is_some() && !is_parser_dstr(arg))
                || matches!(kind::of(arg), kind::SELF_NODE | kind::LOCAL_VARIABLE_READ_NODE | kind::IT_LOCAL_VARIABLE_READ_NODE | kind::INSTANCE_VARIABLE_READ_NODE)
        }
    }
}

/// `receiver == node || receiver.descendants.include?(node)` with rubocop-ast's structural ==.
fn contains_equal(src: &crate::source::Source, receiver: &Node, target: (usize, usize), target_kind: usize) -> bool {
    struct Find<'s> {
        src: &'s crate::source::Source<'s>,
        target: (usize, usize),
        kind: usize,
        found: bool,
    }
    impl<'pr> Visit<'pr> for Find<'_> {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            if kind::of(&node) == self.kind {
                let span = (node.location().start_offset(), node.location().end_offset());
                self.found |= helpers::node_equality::same(self.src, span, self.target);
            }
        }
    }
    let mut f = Find { src, target, kind: target_kind, found: false };
    f.visit(receiver);
    f.found
}
