use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/UnmodifiedReduceAccumulator";
pub static COP: UnmodifiedReduceAccumulator = UnmodifiedReduceAccumulator;
pub struct UnmodifiedReduceAccumulator;

/// Every node of a subtree (the node itself included), in pre-order.
#[derive(Default)]
struct All<'pr>(Vec<Node<'pr>>);

impl<'pr> Visit<'pr> for All<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        self.0.push(node);
    }
    fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
        self.0.push(node);
    }
}

/// `each_node`: the node and its descendants. Typed child fields (StatementsNode, ArgumentsNode,
/// ...) skip the enter hooks, which only drops wrapper nodes no pattern here looks at.
fn each_node<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    let mut all = All::default();
    all.visit(node);
    all.0
}

fn span(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// A parser `send` (not `csend`, no literal block of its own).
fn send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// `(lvar name)`.
fn is_lvar(node: &Node, name: Option<&[u8]>) -> bool {
    name.is_some_and(|n| node.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == n))
}

/// `lvar_used?`: `x`, `x = ...`, `x << ...`, `"#{x}"`.
fn lvar_used(node: &Node, name: Option<&[u8]>) -> bool {
    let Some(n) = name else { return false };
    if is_lvar(node, name) || node.as_local_variable_write_node().is_some_and(|w| w.name().as_slice() == n) {
        return true;
    }
    if let Some(c) = send(node) {
        return method_name(&c) == "<<" && c.receiver().is_some_and(|r| is_lvar(&r, name));
    }
    if let Some(d) = node.as_interpolated_string_node().filter(|_| is_parser_dstr(node)) {
        let parts: Vec<_> = d.parts().iter().collect();
        if let [p] = parts.as_slice()
            && let Some(e) = p.as_embedded_statements_node()
            && let Some(s) = e.statements()
        {
            let stmts: Vec<_> = s.body().iter().collect();
            return matches!(stmts.as_slice(), [v] if is_lvar(v, name));
        }
    }
    false
}

/// `{ivar gvar cvar lvar send}` as a direct argument.
fn variable_or_send(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::INSTANCE_VARIABLE_READ_NODE
            | kind::GLOBAL_VARIABLE_READ_NODE
            | kind::CLASS_VARIABLE_READ_NODE
            | kind::LOCAL_VARIABLE_READ_NODE
    ) || send(node).is_some()
}

/// `element_modified?`: anywhere under the block body, the element is passed to a method with
/// other arguments, has a method called on it with a variable or call argument, or is assigned.
fn element_modified(body: &Node, el: Option<&[u8]>) -> bool {
    let Some(name) = el else { return false };
    each_node(body).iter().any(|n| {
        if let Some(c) = send(n) {
            let args = send_args(&c);
            let m = method_name(&c);
            if m != "[]" && m != "[]=" && args.len() >= 2 && args.iter().any(|a| each_node(a).iter().any(|d| is_lvar(d, el))) {
                return true;
            }
            return c.receiver().is_some_and(|r| is_lvar(&r, el)) && args.iter().any(variable_or_send);
        }
        // `el[i] += x`: parser's op_asgn holds a `send :[]` with the index arguments.
        let index = match kind::of(n) {
            kind::INDEX_OPERATOR_WRITE_NODE => {
                let w = n.as_index_operator_write_node().unwrap();
                Some((w.receiver(), w.arguments(), w.call_operator_loc()))
            }
            kind::INDEX_OR_WRITE_NODE => {
                let w = n.as_index_or_write_node().unwrap();
                Some((w.receiver(), w.arguments(), w.call_operator_loc()))
            }
            kind::INDEX_AND_WRITE_NODE => {
                let w = n.as_index_and_write_node().unwrap();
                Some((w.receiver(), w.arguments(), w.call_operator_loc()))
            }
            kind::INDEX_TARGET_NODE => {
                let w = n.as_index_target_node().unwrap();
                Some((Some(w.receiver()), w.arguments(), None))
            }
            _ => None,
        };
        if let Some((recv, args, op)) = index {
            let csend = op.is_some_and(|o| o.as_slice() == b"&.");
            return !csend
                && recv.is_some_and(|r| is_lvar(&r, el))
                && args.is_some_and(|a| a.arguments().iter().any(|x| variable_or_send(&x)));
        }
        match kind::of(n) {
            kind::LOCAL_VARIABLE_WRITE_NODE => n.as_local_variable_write_node().unwrap().name().as_slice() == name,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE => n.as_local_variable_operator_write_node().unwrap().name().as_slice() == name,
            kind::LOCAL_VARIABLE_OR_WRITE_NODE => n.as_local_variable_or_write_node().unwrap().name().as_slice() == name,
            kind::LOCAL_VARIABLE_AND_WRITE_NODE => n.as_local_variable_and_write_node().unwrap().name().as_slice() == name,
            _ => false,
        }
    })
}

/// A capture of `expression_values`: `None` for a nil capture (`X = 1` has no scope).
fn capture<'a>(node: &Node<'a>) -> Option<Option<Vec<u8>>> {
    let name = |n: ruby_prism::ConstantId<'a>| Some(Some(n.as_slice().to_vec()));
    // A truthy capture that is never a variable name (a node).
    let other = Some(Some(Vec::new()));
    match kind::of(node) {
        kind::LOCAL_VARIABLE_READ_NODE => name(node.as_local_variable_read_node().unwrap().name()),
        kind::INSTANCE_VARIABLE_READ_NODE => name(node.as_instance_variable_read_node().unwrap().name()),
        kind::GLOBAL_VARIABLE_READ_NODE => name(node.as_global_variable_read_node().unwrap().name()),
        kind::CLASS_VARIABLE_READ_NODE => name(node.as_class_variable_read_node().unwrap().name()),
        kind::LOCAL_VARIABLE_WRITE_NODE => name(node.as_local_variable_write_node().unwrap().name()),
        kind::LOCAL_VARIABLE_TARGET_NODE => name(node.as_local_variable_target_node().unwrap().name()),
        kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE => name(node.as_local_variable_operator_write_node().unwrap().name()),
        kind::LOCAL_VARIABLE_OR_WRITE_NODE => name(node.as_local_variable_or_write_node().unwrap().name()),
        kind::LOCAL_VARIABLE_AND_WRITE_NODE => name(node.as_local_variable_and_write_node().unwrap().name()),
        kind::INSTANCE_VARIABLE_WRITE_NODE => name(node.as_instance_variable_write_node().unwrap().name()),
        kind::INSTANCE_VARIABLE_TARGET_NODE => name(node.as_instance_variable_target_node().unwrap().name()),
        kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE => name(node.as_instance_variable_operator_write_node().unwrap().name()),
        kind::INSTANCE_VARIABLE_OR_WRITE_NODE => name(node.as_instance_variable_or_write_node().unwrap().name()),
        kind::INSTANCE_VARIABLE_AND_WRITE_NODE => name(node.as_instance_variable_and_write_node().unwrap().name()),
        kind::CLASS_VARIABLE_WRITE_NODE => name(node.as_class_variable_write_node().unwrap().name()),
        kind::CLASS_VARIABLE_TARGET_NODE => name(node.as_class_variable_target_node().unwrap().name()),
        kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE => name(node.as_class_variable_operator_write_node().unwrap().name()),
        kind::CLASS_VARIABLE_OR_WRITE_NODE => name(node.as_class_variable_or_write_node().unwrap().name()),
        kind::CLASS_VARIABLE_AND_WRITE_NODE => name(node.as_class_variable_and_write_node().unwrap().name()),
        kind::GLOBAL_VARIABLE_WRITE_NODE => name(node.as_global_variable_write_node().unwrap().name()),
        kind::GLOBAL_VARIABLE_TARGET_NODE => name(node.as_global_variable_target_node().unwrap().name()),
        kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE => name(node.as_global_variable_operator_write_node().unwrap().name()),
        kind::GLOBAL_VARIABLE_OR_WRITE_NODE => name(node.as_global_variable_or_write_node().unwrap().name()),
        kind::GLOBAL_VARIABLE_AND_WRITE_NODE => name(node.as_global_variable_and_write_node().unwrap().name()),
        kind::CONSTANT_WRITE_NODE
        | kind::CONSTANT_TARGET_NODE
        | kind::CONSTANT_OPERATOR_WRITE_NODE
        | kind::CONSTANT_OR_WRITE_NODE
        | kind::CONSTANT_AND_WRITE_NODE => Some(None),
        // casgn scope (a const or cbase), masgn's mlhs.
        kind::CONSTANT_PATH_WRITE_NODE
        | kind::CONSTANT_PATH_TARGET_NODE
        | kind::CONSTANT_PATH_OPERATOR_WRITE_NODE
        | kind::CONSTANT_PATH_OR_WRITE_NODE
        | kind::CONSTANT_PATH_AND_WRITE_NODE
        | kind::MULTI_WRITE_NODE => other,
        // `$(send _ _)`: a send without arguments, including those inside parser's op-asgn and mlhs.
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            let no_args = c.arguments().is_none() && c.block().is_none_or(|b| b.as_block_argument_node().is_none());
            (!is_csend(&c) && no_args).then_some(Some(Vec::new()))
        }
        kind::CALL_OPERATOR_WRITE_NODE | kind::CALL_OR_WRITE_NODE | kind::CALL_AND_WRITE_NODE | kind::CALL_TARGET_NODE => {
            let op = match kind::of(node) {
                kind::CALL_OPERATOR_WRITE_NODE => node.as_call_operator_write_node().unwrap().call_operator_loc(),
                kind::CALL_OR_WRITE_NODE => node.as_call_or_write_node().unwrap().call_operator_loc(),
                kind::CALL_AND_WRITE_NODE => node.as_call_and_write_node().unwrap().call_operator_loc(),
                _ => Some(node.as_call_target_node().unwrap().call_operator_loc()),
            };
            (op.is_none_or(|o| o.as_slice() != b"&.")).then_some(Some(Vec::new()))
        }
        _ => None,
    }
}

/// `acceptable_return?`: the return value depends on something besides the element.
fn acceptable_return(node: &Node, el: Option<&[u8]>) -> bool {
    let truthy: Vec<Vec<u8>> = each_node(node).iter().filter_map(capture).flatten().collect();
    // `vars.none? || (vars - [element_name]).any?`
    truthy.is_empty() || truthy.iter().any(|v| v.is_empty() || Some(v.as_slice()) != el)
}

/// Parser argument names of a block (`argument_list`), in order; `None` for anonymous ones.
fn argument_names(block: &ruby_prism::BlockNode) -> Vec<Option<Vec<u8>>> {
    let Some(params) = block.parameters() else { return vec![] };
    if let Some(n) = params.as_numbered_parameters_node() {
        return (1..=n.maximum()).map(|i| Some(format!("_{i}").into_bytes())).collect();
    }
    let Some(bp) = params.as_block_parameters_node() else { return vec![] };
    let mut out: Vec<(usize, Option<Vec<u8>>)> = vec![];
    for n in each_node(&bp.as_node()) {
        let name = |id: Option<ruby_prism::ConstantId>| id.map(|i| i.as_slice().to_vec());
        let entry = match kind::of(&n) {
            kind::REQUIRED_PARAMETER_NODE => name(Some(n.as_required_parameter_node().unwrap().name())),
            kind::OPTIONAL_PARAMETER_NODE => name(Some(n.as_optional_parameter_node().unwrap().name())),
            kind::REST_PARAMETER_NODE => name(n.as_rest_parameter_node().unwrap().name()),
            kind::SPLAT_NODE => match n.as_splat_node().unwrap().expression() {
                Some(e) => e.as_required_parameter_node().map(|r| r.name().as_slice().to_vec()),
                None => None,
            },
            kind::REQUIRED_KEYWORD_PARAMETER_NODE => name(Some(n.as_required_keyword_parameter_node().unwrap().name())),
            kind::OPTIONAL_KEYWORD_PARAMETER_NODE => name(Some(n.as_optional_keyword_parameter_node().unwrap().name())),
            kind::KEYWORD_REST_PARAMETER_NODE => name(n.as_keyword_rest_parameter_node().unwrap().name()),
            kind::BLOCK_PARAMETER_NODE => name(n.as_block_parameter_node().unwrap().name()),
            kind::BLOCK_LOCAL_VARIABLE_NODE => name(Some(n.as_block_local_variable_node().unwrap().name())),
            _ => continue,
        };
        out.push((n.location().start_offset(), entry));
    }
    out.sort_by_key(|(s, _)| *s);
    out.into_iter().map(|(_, n)| n).collect()
}

impl Cop for UnmodifiedReduceAccumulator {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if !matches!(method, "reduce" | "inject") {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        // `it` blocks are itblocks, which the pattern does not match.
        if block.parameters().is_some_and(|p| p.as_it_parameters_node().is_some()) {
            return;
        }
        let Some(body) = block.body() else { return };
        let names = argument_names(&block);
        if names.len() < 2 {
            return;
        }
        let (acc, el) = (names[0].as_deref(), names[1].as_deref());

        // `return_values`: the last expression, plus the first argument of each `next`/`break`
        // whose nearest block is this one.
        let mut values: Vec<(Node, (usize, usize))> = vec![];
        match body.as_statements_node() {
            Some(s) => {
                let last = s.body().iter().last().unwrap();
                let r = span(&last);
                values.push((last, r));
            }
            None => {
                let r = body_range(&body);
                values.push((body, r));
            }
        }
        let all = each_node(&block.body().unwrap());
        let nested: Vec<(usize, usize)> = all
            .iter()
            .filter(|n| match kind::of(n) {
                kind::LAMBDA_NODE => true,
                kind::CALL_NODE => n.as_call_node().unwrap().block().is_some_and(|b| b.as_block_node().is_some()),
                kind::SUPER_NODE => n.as_super_node().unwrap().block().is_some_and(|b| b.as_block_node().is_some()),
                kind::FORWARDING_SUPER_NODE => n.as_forwarding_super_node().unwrap().block().is_some(),
                _ => false,
            })
            .map(span)
            .collect();
        // `each_descendant` excludes parser's body node itself (a lone statement).
        let lone = block.body().and_then(|b| b.as_statements_node()).filter(|s| s.body().iter().count() == 1).map(|_| values[0].1);
        for n in all {
            if Some(span(&n)) == lone && kind::of(&n) != kind::STATEMENTS_NODE {
                continue;
            }
            let args = match kind::of(&n) {
                kind::NEXT_NODE => n.as_next_node().unwrap().arguments(),
                kind::BREAK_NODE => n.as_break_node().unwrap().arguments(),
                _ => continue,
            };
            let (s, e) = span(&n);
            if nested.iter().any(|&(a, b)| a <= s && e <= b) {
                continue;
            }
            if let Some(first) = args.and_then(|a| a.arguments().iter().next()) {
                let r = span(&first);
                values.push((first, r));
            }
        }

        let accum = acc.map(|a| String::from_utf8_lossy(a).into_owned()).unwrap_or_default();
        // `returned_accumulator_index`
        let index = values.iter().find(|(v, _)| {
            let Some(c) = send(v) else { return false };
            if !c.receiver().is_some_and(|r| is_lvar(&r, acc)) {
                return false;
            }
            match method_name(&c) {
                "[]=" => true,
                "[]" => !send_args(&c).iter().any(|a| lvar_used(a, el)),
                _ => false,
            }
        });
        if let Some((_, (s, e))) = index {
            cx.add(*s, *e, format!("Do not return an element of the accumulator in `{method}`."));
            return;
        }
        let body = block.body().unwrap();
        if element_modified(&body, el) || values.iter().any(|(v, _)| lvar_used(v, acc)) {
            return;
        }
        for (v, (s, e)) in &values {
            if !acceptable_return(v, el) {
                cx.add(*s, *e, format!("Ensure the accumulator `{accum}` will be modified by `{method}`."));
            }
        }
    }
}
