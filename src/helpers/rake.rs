//! rubocop-rake's helpers: TaskName, ClassDefinition, TaskDefinition and the cops' `namespaces`.

use crate::helpers::node_types::plain_block_params;
use crate::helpers::parser_parent::is_top_const;
use crate::helpers::thread_safety::{Anc, parser_ancestors};
use crate::prelude::*;
use ruby_prism::CallNode;

/// The value of a parser `sym` or `str` node.
fn sym_or_str(node: &Node, target_ruby: f64) -> Option<String> {
    let bytes = if let Some(s) = node.as_symbol_node() {
        if is_parser_dsym(node, target_ruby) {
            return None;
        }
        s.unescaped().to_vec()
    } else if let Some(s) = node.as_string_node() {
        if is_parser_dstr(node) {
            return None;
        }
        s.unescaped().to_vec()
    } else {
        return None;
    };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Helper::TaskName.task_name: the first argument's symbol or string, or the key of a one-pair hash.
pub fn task_name(call: &CallNode, target_ruby: f64) -> Option<String> {
    let first = send_args(call).into_iter().next()?;
    let elements: Vec<Node> = if let Some(h) = first.as_hash_node() {
        h.elements().iter().collect()
    } else if let Some(h) = first.as_keyword_hash_node() {
        h.elements().iter().collect()
    } else {
        return sym_or_str(&first, target_ruby);
    };
    let [pair] = elements.as_slice() else { return None };
    // `pair.children.first`: a pair's key, or a kwsplat's operand.
    let key = match pair.as_assoc_node() {
        Some(a) => a.key(),
        None => pair.as_assoc_splat_node()?.value()?,
    };
    sym_or_str(&key, target_ruby)
}

/// The names of the `namespace` blocks around `call` (its own block included), nearest first: the
/// cops' `node.each_ancestor(:block)` over `send_node.method?(:namespace)`.
pub fn namespaces(call: &CallNode, ancestors: &[Node], target_ruby: f64) -> Vec<Option<String>> {
    let own = call.block().and_then(|b| b.as_block_node()).is_some_and(|b| plain_block_params(b.parameters()));
    let mut ns = Vec::new();
    if own && method_name(call) == "namespace" {
        ns.push(task_name(call, target_ruby));
    }
    for c in parser_ancestors(ancestors).iter().filter(|a| a.is_block()).filter_map(Anc::block_call) {
        if method_name(&c) == "namespace" {
            ns.push(task_name(&c, target_ruby));
        }
    }
    ns
}

/// Helper::ClassDefinition.in_class_definition? || !Helper::TaskDefinition.in_task_or_namespace?
/// negated: the node (class, module or def) sits in a task or namespace block and not in a class.
pub fn in_task_outside_class(ancestors: &[Node]) -> bool {
    let anc = parser_ancestors(ancestors);
    let class_definition = anc.iter().any(|a| match a {
        Anc::Node(n) => matches!(n, Node::ClassNode { .. } | Node::ModuleNode { .. } | Node::SingletonClassNode { .. }),
        // (block (send (const {nil? cbase} {:Class :Module}) :new) args _)
        Anc::Block { plain: true, .. } => a.block_call().is_some_and(|c| {
            !is_csend(&c)
                && method_name(&c) == "new"
                && send_args(&c).is_empty()
                && c.receiver().is_some_and(|r| is_top_const(&r, "Class") || is_top_const(&r, "Module"))
        }),
        Anc::Block { .. } => false,
    });
    // (block (send _ {:task :namespace} ...) args _)
    !class_definition
        && anc.iter().filter(|a| a.is_block()).filter_map(Anc::block_call).any(|c| {
            !is_csend(&c) && matches!(method_name(&c), "task" | "namespace")
        })
}

/// `(send nil? :<name> ...)`
pub fn is_receiverless(call: &CallNode, name: &str) -> bool {
    call.receiver().is_none() && !is_csend(call) && method_name(call) == name
}

/// DuplicateTask/DuplicateNamespace `source_location`: the smart path and line of `start`.
pub fn source_location(cx: &Cx, start: usize) -> String {
    let path = crate::config::validator::smart_path(std::path::Path::new(&cx.src.absolute_path()), &crate::config::cwd());
    format!("{path}:{}", cx.src.line_of(start))
}
