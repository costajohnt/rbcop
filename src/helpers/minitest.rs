//! rubocop-minitest's mixins: MinitestExplorationHelpers (test classes, test cases, assertions),
//! ArgumentRangeHelper, and the parser-node shapes its patterns match.

use crate::prelude::*;
use helpers::visibility_help::{node_visibility, non_public_modifier};
use ruby_prism::{CallNode, ClassNode, DefNode, Visit};

/// `/re/ =~ x` with a static regexp: parser `match_with_lvasgn`, not a send.
fn match_with_lvasgn(call: &CallNode) -> bool {
    call.name().as_slice() == b"=~"
        && call.call_operator_loc().is_none()
        && call.receiver().is_some_and(|r| r.as_regular_expression_node().is_some())
}

/// A CallNode the parser gem makes a `send`/`csend` (no literal block, not `/re/ =~ x`, not a
/// folded `- 1`).
pub fn as_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    (call.block().is_none_or(|b| b.as_block_node().is_none())
        && !match_with_lvasgn(&call)
        && helpers::literal_node::folded_numeric(node).is_none())
    .then_some(call)
}

/// A parser `send` (`call_type?` minus csend).
pub fn as_send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    as_call(node).filter(|c| !is_csend(c))
}

/// The visited CallNode as `(send nil? :name ...)` with one of `names`: its parser arguments.
pub fn receiverless<'pr>(call: &CallNode<'pr>, names: &[&str]) -> Option<Vec<Node<'pr>>> {
    (call.receiver().is_none() && names.contains(&method_name(call))).then(|| send_args(call))
}

pub fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// ArgumentRangeHelper#first_and_second_arguments_range.
pub fn first_and_second(args: &[Node]) -> (usize, usize) {
    (range(&args[0]).0, range(&args[1]).1)
}

/// ArgumentRangeHelper#all_arguments_range.
pub fn all_arguments(args: &[Node]) -> (usize, usize) {
    (range(&args[0]).0, range(args.last().unwrap()).1)
}

/// The parser receiver of a send (`self` when it has none), as MinitestCopRule#correct_receiver.
pub fn receiver_source<'pr>(call: &CallNode<'pr>) -> &'pr str {
    call.receiver().map_or("self", |r| node_text(&r))
}

/// `(sym :name)`.
pub fn is_sym(node: &Node, name: &str) -> bool {
    node.as_symbol_node().is_some_and(|s| s.opening_loc().is_some_and(|o| o.as_slice() == b":") && s.unescaped() == name.as_bytes())
}

pub fn is_regexp(node: &Node) -> bool {
    matches!(node, Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. })
}

/// `const` (ConstantReadNode / ConstantPathNode).
pub fn is_const(node: &Node) -> bool {
    matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })
}

/// Structural equality of two argument nodes (rubocop-ast `==`).
pub fn same(cx: &Cx, a: &Node, b: &Node) -> bool {
    helpers::node_equality::same(cx.src, range(a), range(b))
}

/// The message argument of a MinitestCopRule/PredicateAssertionHandleable offense:
/// `arguments.last if arguments.first != arguments.last`.
pub fn message_arg<'a, 'pr>(cx: &Cx, args: &'a [Node<'pr>]) -> Option<&'a Node<'pr>> {
    let (first, last) = (args.first()?, args.last()?);
    (!same(cx, first, last)).then_some(last)
}

/// A literal-block call that is a parser `block` (not numblock/itblock): the call.
pub fn block_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    let block = call.block()?.as_block_node()?;
    (!matches!(block.parameters(), Some(Node::NumberedParametersNode { .. }) | Some(Node::ItParametersNode { .. })))
        .then_some(call)
}

/// A call owning a literal block of any kind (parser `any_block`).
pub fn any_block_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let call = node.as_call_node()?;
    call.block()?.as_block_node()?;
    Some(call)
}

/// The parser body of a def/block (`StatementsNode` with one statement unwrapped).
pub fn parser_body<'pr>(body: Option<Node<'pr>>) -> Option<Node<'pr>> {
    let body = body?;
    match body.as_statements_node() {
        Some(s) => {
            let mut it = s.body().iter();
            match (it.next(), it.next()) {
                (Some(only), None) => Some(only),
                (None, _) => None,
                _ => Some(body),
            }
        }
        None => Some(body),
    }
}

/// A Prism StatementsNode standing for a parser `begin` (two or more statements).
pub fn is_begin(node: &Node) -> bool {
    node.as_statements_node().is_some_and(|s| s.body().iter().count() > 1)
}

fn stmts(node: Option<Node>) -> Vec<Node> {
    match node.and_then(|n| n.as_statements_node()) {
        Some(s) => s.body().iter().collect(),
        None => Vec::new(),
    }
}

/// `test_class?`
pub fn test_class(class: &ClassNode) -> bool {
    class.superclass().is_some() && node_text(&class.constant_path()).ends_with("Test")
}

/// `class_def_nodes`: the `def`s directly in a class body, with that body's statements (the
/// def's parser siblings when there are two or more) and its index there.
pub fn class_def_nodes<'pr>(class: &ClassNode<'pr>) -> (Vec<Node<'pr>>, Vec<usize>) {
    // ponytail: a class body with rescue/ensure is a parser `rescue`/`ensure`; treated as no defs.
    let body = stmts(class.body());
    let idx = (0..body.len()).filter(|&i| body[i].as_def_node().is_some_and(|d| d.receiver().is_none())).collect();
    (body, idx)
}

/// `non_public?` for a def among its class body statements (parent is the body or the class).
pub fn non_public_in(body: &[Node], i: usize) -> bool {
    let sibs: &[Node] = if body.len() > 1 { body } else { &[] };
    node_visibility(&body[i], None, sibs, i) != "public"
}

/// `non_public?` for a def given its Prism ancestors.
pub fn non_public_at(def: &Node, ancestors: &[Node]) -> bool {
    let parent = helpers::parser_parent::parser_parent(ancestors);
    let (sibs, idx) = helpers::visibility_help::siblings(ancestors, def);
    non_public_modifier(parent) || node_visibility(def, parent, &sibs, idx) != "public"
}

/// `test_method?` without the visibility check: `test_` name, no arguments.
pub fn test_method_shape(def: &DefNode) -> bool {
    def.name().as_slice().starts_with(b"test_") && def.parameters().is_none()
}

pub fn is_test_block(call: &CallNode) -> bool {
    matches!(method_name(call), "test" | "it")
}

/// `test_blocks`: parser `block`s calling `test`/`it` under a node, not inside a def (pre-order).
pub fn test_blocks<'pr>(node: &Node<'pr>) -> Vec<CallNode<'pr>> {
    struct V<'pr>(Vec<CallNode<'pr>>);
    impl<'pr> Visit<'pr> for V<'pr> {
        fn visit_def_node(&mut self, _: &DefNode<'pr>) {}
        fn visit_call_node(&mut self, c: &CallNode<'pr>) {
            if let Some(b) = block_call(&c.as_node())
                && is_test_block(&b)
            {
                self.0.push(b);
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut v = V(Vec::new());
    v.visit(node);
    v.0
}

/// A test case: a test method or a test block (the block's call).
pub enum TestCase<'pr> {
    Def(DefNode<'pr>),
    Block(CallNode<'pr>),
}

impl<'pr> TestCase<'pr> {
    pub fn node(&self) -> Node<'pr> {
        match self {
            TestCase::Def(d) => d.as_node(),
            TestCase::Block(c) => c.as_node(),
        }
    }
}

/// `test_cases(class_node, visibility_check:)`.
pub fn test_cases<'pr>(class: &ClassNode<'pr>, visibility_check: bool) -> Vec<TestCase<'pr>> {
    let (body, idx) = class_def_nodes(class);
    let mut out: Vec<TestCase> = idx
        .into_iter()
        .filter(|&i| {
            let d = body[i].as_def_node().unwrap();
            test_method_shape(&d) && !(visibility_check && non_public_in(&body, i))
        })
        .map(|i| TestCase::Def(body[i].as_def_node().unwrap()))
        .collect();
    out.extend(test_blocks(&class.as_node()).into_iter().map(TestCase::Block));
    out
}

const VALUE_MATCHERS: &[&str] = &[
    "must_be_empty", "must_equal", "must_be_close_to", "must_be_within_delta", "must_be_within_epsilon",
    "must_include", "must_be_instance_of", "must_be_kind_of", "must_match", "must_be_nil", "must_be",
    "must_respond_to", "must_be_same_as", "path_must_exist", "path_wont_exist", "wont_be_empty", "wont_equal",
    "wont_be_close_to", "wont_be_within_delta", "wont_be_within_epsilon", "wont_include", "wont_be_instance_of",
    "wont_be_kind_of", "wont_match", "wont_be_nil", "wont_be", "wont_respond_to", "wont_be_same_as",
];
const BLOCK_MATCHERS: &[&str] =
    &["must_output", "must_pattern_match", "must_raise", "must_be_silent", "must_throw", "wont_pattern_match"];

pub fn is_value_matcher(name: &str) -> bool {
    VALUE_MATCHERS.contains(&name)
}
pub fn is_block_matcher(name: &str) -> bool {
    BLOCK_MATCHERS.contains(&name)
}
pub fn is_matcher(name: &str) -> bool {
    is_value_matcher(name) || is_block_matcher(name)
}

pub const LIFECYCLE_HOOKS: &[&str] = &["before_setup", "setup", "after_setup", "before_teardown", "teardown", "after_teardown"];

/// `lifecycle_hook_method?`
pub fn lifecycle_hook(node: &Node) -> bool {
    node.as_def_node().is_some_and(|d| d.receiver().is_none() && LIFECYCLE_HOOKS.contains(&std::str::from_utf8(d.name().as_slice()).unwrap_or("")))
}

/// The value of an assignment that responds to `expression` (lvasgn ivasgn cvasgn gvasgn casgn
/// masgn op_asgn or_asgn and_asgn).
fn assignment_value<'pr>(node: &Node<'pr>) -> Option<Option<Node<'pr>>> {
    let v = match node {
        Node::LocalVariableWriteNode { .. } => node.as_local_variable_write_node().unwrap().value(),
        Node::InstanceVariableWriteNode { .. } => node.as_instance_variable_write_node().unwrap().value(),
        Node::ClassVariableWriteNode { .. } => node.as_class_variable_write_node().unwrap().value(),
        Node::GlobalVariableWriteNode { .. } => node.as_global_variable_write_node().unwrap().value(),
        Node::ConstantWriteNode { .. } => node.as_constant_write_node().unwrap().value(),
        Node::ConstantPathWriteNode { .. } => node.as_constant_path_write_node().unwrap().value(),
        Node::MultiWriteNode { .. } => node.as_multi_write_node().unwrap().value(),
        Node::LocalVariableOperatorWriteNode { .. } => node.as_local_variable_operator_write_node().unwrap().value(),
        Node::LocalVariableOrWriteNode { .. } => node.as_local_variable_or_write_node().unwrap().value(),
        Node::LocalVariableAndWriteNode { .. } => node.as_local_variable_and_write_node().unwrap().value(),
        Node::InstanceVariableOperatorWriteNode { .. } => node.as_instance_variable_operator_write_node().unwrap().value(),
        Node::InstanceVariableOrWriteNode { .. } => node.as_instance_variable_or_write_node().unwrap().value(),
        Node::InstanceVariableAndWriteNode { .. } => node.as_instance_variable_and_write_node().unwrap().value(),
        Node::ClassVariableOperatorWriteNode { .. } => node.as_class_variable_operator_write_node().unwrap().value(),
        Node::ClassVariableOrWriteNode { .. } => node.as_class_variable_or_write_node().unwrap().value(),
        Node::ClassVariableAndWriteNode { .. } => node.as_class_variable_and_write_node().unwrap().value(),
        Node::GlobalVariableOperatorWriteNode { .. } => node.as_global_variable_operator_write_node().unwrap().value(),
        Node::GlobalVariableOrWriteNode { .. } => node.as_global_variable_or_write_node().unwrap().value(),
        Node::GlobalVariableAndWriteNode { .. } => node.as_global_variable_and_write_node().unwrap().value(),
        Node::ConstantOperatorWriteNode { .. } => node.as_constant_operator_write_node().unwrap().value(),
        Node::ConstantOrWriteNode { .. } => node.as_constant_or_write_node().unwrap().value(),
        Node::ConstantAndWriteNode { .. } => node.as_constant_and_write_node().unwrap().value(),
        Node::ConstantPathOperatorWriteNode { .. } => node.as_constant_path_operator_write_node().unwrap().value(),
        Node::ConstantPathOrWriteNode { .. } => node.as_constant_path_or_write_node().unwrap().value(),
        Node::ConstantPathAndWriteNode { .. } => node.as_constant_path_and_write_node().unwrap().value(),
        Node::CallOperatorWriteNode { .. } => node.as_call_operator_write_node().unwrap().value(),
        Node::CallOrWriteNode { .. } => node.as_call_or_write_node().unwrap().value(),
        Node::CallAndWriteNode { .. } => node.as_call_and_write_node().unwrap().value(),
        Node::IndexOperatorWriteNode { .. } => node.as_index_operator_write_node().unwrap().value(),
        Node::IndexOrWriteNode { .. } => node.as_index_or_write_node().unwrap().value(),
        Node::IndexAndWriteNode { .. } => node.as_index_and_write_node().unwrap().value(),
        _ => return None,
    };
    Some(Some(v))
}

/// The parser method name of a send or any_block node, with its receiver.
fn send_or_block<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    if let Some(c) = any_block_call(node) {
        return Some(c);
    }
    as_send(node)
}

/// `assertion_method?`
pub fn assertion_method(node: Option<&Node>) -> bool {
    let Some(node) = node else { return false };
    if let Some(v) = assignment_value(node) {
        return assertion_method(v.as_ref());
    }
    let Some(call) = send_or_block(node) else { return false };
    let name = method_name(&call);
    (call.receiver().is_none() && (name.starts_with("assert") || name.starts_with("refute"))) || name == "flunk" || is_matcher(name)
}

/// The parser `send` descendants of a node (excluding the node itself unless `with_self`), as
/// `each_descendant(:send)`: a call with a literal block contributes its send.
pub fn descendant_sends<'pr>(node: &Node<'pr>, with_self: bool) -> Vec<CallNode<'pr>> {
    struct V<'pr>(Vec<CallNode<'pr>>);
    impl<'pr> Visit<'pr> for V<'pr> {
        fn visit_call_node(&mut self, c: &CallNode<'pr>) {
            let n = c.as_node();
            if !is_csend(c) && !match_with_lvasgn(c) && helpers::literal_node::folded_numeric(&n).is_none() {
                self.0.push(c.as_node().as_call_node().unwrap());
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut v = V(Vec::new());
    v.visit(node);
    let start = node.location().start_offset();
    let end = node.location().end_offset();
    if !with_self {
        // The node itself (a send, or the send of a block) is not its own descendant.
        if node.as_call_node().is_some() {
            v.0.retain(|c| (c.location().start_offset(), c.location().end_offset()) != (start, end));
        }
    }
    v.0
}

/// `assertion_method?` on a parser send (the send of a block literal included).
pub fn send_is_assertion(call: &CallNode) -> bool {
    let name = method_name(call);
    (call.receiver().is_none() && (name.starts_with("assert") || name.starts_with("refute"))) || name == "flunk" || is_matcher(name)
}

/// `assertions_count(node)`: assertion sends among the node's descendants.
pub fn assertions_count(node: &Node) -> usize {
    descendant_sends(node, false).iter().filter(|c| send_is_assertion(c)).count()
}

/// The parser children of a node that are `send`s (`each_child_node(:send)`), for the node kinds a
/// def body usually is.
fn child_sends<'pr>(node: &Node<'pr>) -> Vec<CallNode<'pr>> {
    let one = |n: Option<Node<'pr>>| -> Vec<Node<'pr>> { parser_body(n).into_iter().collect() };
    let kids: Vec<Node> = match node {
        Node::StatementsNode { .. } => node.as_statements_node().unwrap().body().iter().collect(),
        Node::CallNode { .. } if any_block_call(node).is_some() => {
            // (block (send ...) args body): the send is a child.
            let c = node.as_call_node().unwrap();
            return if is_csend(&c) { Vec::new() } else { vec![c] };
        }
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            let mut k = vec![n.predicate()];
            k.extend(one(n.statements().map(|s| s.as_node())));
            if let Some(e) = n.subsequent() {
                match e.as_else_node() {
                    Some(e) => k.extend(one(e.statements().map(|s| s.as_node()))),
                    None => k.push(e),
                }
            }
            k
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            let mut k = vec![n.predicate()];
            k.extend(one(n.statements().map(|s| s.as_node())));
            if let Some(e) = n.else_clause() {
                k.extend(one(e.statements().map(|s| s.as_node())));
            }
            k
        }
        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            vec![n.left(), n.right()]
        }
        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            vec![n.left(), n.right()]
        }
        _ => match assignment_value(node) {
            Some(Some(v)) => vec![v],
            _ => Vec::new(),
        },
    };
    kids.iter().filter_map(as_send).collect()
}

/// `assertions(def_node)`: the def body if it is an assertion send, else its child sends that are
/// assertions.
pub fn assertions<'pr>(def: &DefNode<'pr>) -> Vec<CallNode<'pr>> {
    body_assertions(def.body())
}

/// `assertions` on a def or block body (Prism body).
pub fn body_assertions<'pr>(body: Option<Node<'pr>>) -> Vec<CallNode<'pr>> {
    let Some(body) = parser_body(body) else { return Vec::new() };
    let sends = match as_send(&body) {
        Some(s) => vec![s],
        None => child_sends(&body),
    };
    sends.into_iter().filter(send_is_assertion).collect()
}

/// MultipleAssertions#assertions_count: assertions in a parser body, counting only the branch of a
/// conditional (or rescue) with the most.
pub fn multiple_assertions_count(node: &Node) -> usize {
    struct C(usize);
    fn count(n: Option<&Node>) -> usize {
        let Some(n) = n else { return 0 };
        let mut c = C(0);
        c.visit(n);
        c.0
    }
    fn body(s: Option<ruby_prism::StatementsNode>) -> Option<Node> {
        s.map(|s| s.as_node())
    }
    // `by_type`: the count without the node's own assertion.
    fn by_type(n: &Node) -> usize {
        let own = usize::from(assertion_method(Some(n)));
        count(Some(n)) - own
    }
    fn if_branches<'pr>(n: &ruby_prism::IfNode<'pr>, out: &mut Vec<Option<Node<'pr>>>) {
        out.push(body(n.statements()));
        match n.subsequent() {
            Some(e) if e.as_if_node().is_some() => if_branches(&e.as_if_node().unwrap(), out),
            Some(e) => out.push(e.as_else_node().and_then(|e| body(e.statements()))),
            None => {}
        }
    }
    fn max(v: Vec<Option<Node>>) -> usize {
        v.iter().map(|n| count(n.as_ref())).max().unwrap_or(0)
    }
    impl<'pr> Visit<'pr> for C {
        fn visit_if_node(&mut self, n: &ruby_prism::IfNode<'pr>) {
            let mut b = Vec::new();
            if_branches(n, &mut b);
            self.0 += max(b);
        }
        fn visit_unless_node(&mut self, n: &ruby_prism::UnlessNode<'pr>) {
            let mut b = vec![body(n.statements())];
            if let Some(e) = n.else_clause() {
                b.push(body(e.statements()));
            }
            self.0 += max(b);
        }
        fn visit_case_node(&mut self, n: &ruby_prism::CaseNode<'pr>) {
            let mut b: Vec<Option<Node>> =
                n.conditions().iter().map(|w| w.as_when_node().and_then(|w| body(w.statements()))).collect();
            if let Some(e) = n.else_clause() {
                b.push(body(e.statements()));
            }
            self.0 += max(b);
        }
        fn visit_case_match_node(&mut self, n: &ruby_prism::CaseMatchNode<'pr>) {
            let mut b: Vec<Option<Node>> =
                n.conditions().iter().map(|w| w.as_in_node().and_then(|w| body(w.statements()))).collect();
            if let Some(e) = n.else_clause() {
                b.push(body(e.statements()));
            }
            self.0 += max(b);
        }
        fn visit_begin_node(&mut self, n: &ruby_prism::BeginNode<'pr>) {
            let main = count(body(n.statements()).as_ref());
            match n.rescue_clause() {
                Some(r) => {
                    let mut b = Vec::new();
                    let mut cur = Some(r);
                    while let Some(r) = cur {
                        b.push(body(r.statements()));
                        cur = r.subsequent();
                    }
                    if let Some(e) = n.else_clause() {
                        b.push(body(e.statements()));
                    }
                    self.0 += main + max(b);
                }
                None => self.0 += main,
            }
            if let Some(e) = n.ensure_clause() {
                self.0 += count(body(e.statements()).as_ref());
            }
        }
        fn visit_rescue_modifier_node(&mut self, n: &ruby_prism::RescueModifierNode<'pr>) {
            self.0 += count(Some(&n.expression())) + count(Some(&n.rescue_expression()));
        }
        fn visit_lambda_node(&mut self, n: &ruby_prism::LambdaNode<'pr>) {
            self.0 += count(n.body().as_ref());
        }
        fn visit_multi_write_node(&mut self, n: &ruby_prism::MultiWriteNode<'pr>) {
            let rhs = n.value();
            let inner = match &rhs {
                Node::ArrayNode { .. } => rhs.as_array_node().unwrap().elements().iter().map(|e| by_type(&e)).sum(),
                _ if as_send(&rhs).is_some() => usize::from(assertion_method(Some(&rhs))),
                _ => 0,
            };
            self.0 += inner + usize::from(assertion_method(Some(&rhs)));
        }
        fn visit_call_node(&mut self, c: &CallNode<'pr>) {
            let n = c.as_node();
            self.0 += usize::from(assertion_method(Some(&n)));
            if let Some(b) = c.block().and_then(|b| b.as_block_node()) {
                self.0 += count(b.body().as_ref());
                return;
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    count(Some(node))
}

/// rubocop-ast `heredoc?` on a str/dstr/xstr node.
pub fn is_heredoc_node(node: &Node) -> bool {
    is_heredoc(node)
}

/// A class ancestor (nearest first) of the node with these Prism ancestors.
pub fn nearest_class<'a, 'pr>(ancestors: &'a [Node<'pr>]) -> Option<ClassNode<'pr>> {
    ancestors.iter().rev().find_map(|a| a.as_class_node())
}

/// `test_case?` for a node with its Prism ancestors.
pub fn test_case(node: &Node, ancestors: &[Node]) -> bool {
    let ok = match node {
        Node::DefNode { .. } => {
            let d = node.as_def_node().unwrap();
            d.receiver().is_none() && test_method_shape(&d) && !non_public_at(node, ancestors)
        }
        Node::CallNode { .. } => block_call(node).is_some_and(|c| is_test_block(&c)),
        _ => false,
    };
    ok && nearest_class(ancestors).is_some_and(|c| test_class(&c))
}

/// MinitestCopRule#define_rule's on_send (`targets` empty: AssertEmpty/RefuteEmpty's own on_send,
/// whose inner call takes no arguments and is `empty?`).
pub fn cop_rule(cx: &mut Cx, node: &Node, assertion: &str, targets: &[&str], preferred: &str, inverse: bool) {
    let call = node.as_call_node().unwrap();
    if method_name(&call) != assertion {
        return;
    }
    let args = send_args(&call);
    let Some(inner) = args.first().and_then(as_call) else { return };
    let inner_args = send_args(&inner);
    let name = method_name(&inner);
    if targets.is_empty() {
        if name != "empty?" || !inner_args.is_empty() {
            return;
        }
    } else if inner_args.is_empty() || !targets.contains(&name) {
        return;
    }
    let mut new_args: Vec<&str> = vec![receiver_source(&inner)];
    if let Some(a) = inner_args.first() {
        new_args.push(node_text(a));
    }
    if inverse {
        new_args.reverse();
    }
    let joined = new_args.join(", ");
    let mut shown = new_args.clone();
    if let Some(m) = message_arg(cx, &args) {
        shown.push(node_text(m));
    }
    let msg = format!("Prefer using `{preferred}({})`.", shown.join(", "));
    let sel = call.message_loc().unwrap();
    let (sel_s, sel_e) = (sel.start_offset(), sel.end_offset());
    let first = range(&args[0]);
    let (s, e) = send_range(&call);
    cx.add_fix(s, e, msg, |c| {
        c.replace(sel_s, sel_e, preferred);
        c.replace(first.0, first.1, &joined);
    });
}

/// `(send $_ op $_)` with a receiver and one argument (`ops`: any of these names).
pub fn binop<'pr>(node: &Node<'pr>, ops: &[&str]) -> Option<(Node<'pr>, Node<'pr>)> {
    let c = as_send(node)?;
    let args = send_args(&c);
    (ops.contains(&method_name(&c)) && args.len() == 1).then_some(())?;
    Some((c.receiver()?, args.into_iter().next().unwrap()))
}

/// `(send $_ name)` with a receiver and no arguments: the receiver.
pub fn unary_send<'pr>(node: &Node<'pr>, name: &str) -> Option<Node<'pr>> {
    let c = as_send(node)?;
    (method_name(&c) == name && send_args(&c).is_empty()).then_some(())?;
    c.receiver()
}

/// One alternative of an equality/match pattern: `(send nil? :outer (send $_ op $_) $...)` or
/// `(send nil? :outer $_ (sym :op) $_ $...)`.
pub enum Form {
    Bin(&'static str, &'static [&'static str]),
    Op(&'static str, &'static str),
}

/// AssertEqual / AssertMatch / RefuteEqual / RefuteMatch.
pub fn compare_cop(cx: &mut Cx, node: &Node, preferred: &str, forms: &[Form], regexp_first: bool) {
    let call = node.as_call_node().unwrap();
    if call.receiver().is_some() {
        return;
    }
    let name = method_name(&call);
    let args = send_args(&call);
    // (expected, actual, index of the first rest argument, replaced range is the first argument)
    let mut found = None;
    for f in forms {
        match f {
            Form::Bin(outer, ops) if *outer == name => {
                if let Some((e, a)) = args.first().and_then(|f| binop(f, ops)) {
                    found = Some((range(&e), range(&a), is_regexp(&a), 1, true));
                }
            }
            Form::Op(outer, op) if *outer == name => {
                if args.len() >= 3 && is_sym(&args[1], op) {
                    found = Some((range(&args[0]), range(&args[2]), is_regexp(&args[2]), 3, false));
                }
            }
            _ => {}
        }
        if found.is_some() {
            break;
        }
    }
    let Some((expected, actual, actual_regexp, rest, first_only)) = found else { return };
    let text = |r: (usize, usize)| &cx.src.text[r.0..r.1];
    let basic = if regexp_first && actual_regexp {
        format!("{}, {}", text(actual), text(expected))
    } else {
        format!("{}, {}", text(expected), text(actual))
    };
    let shown = match args.get(rest) {
        Some(m) => format!("{basic}, {}", node_text(m)),
        None => basic.clone(),
    };
    let msg = format!("Prefer using `{preferred}({shown})`.");
    let sel = call.message_loc().unwrap();
    let (sel_s, sel_e) = (sel.start_offset(), sel.end_offset());
    let r = if first_only { range(&args[0]) } else { (range(&args[0]).0, range(&args[2]).1) };
    let (s, e) = send_range(&call);
    cx.add_fix(s, e, msg, |c| {
        c.replace(sel_s, sel_e, preferred);
        c.replace(r.0, r.1, &basic);
    });
}

fn selector(call: &CallNode) -> (usize, usize) {
    let l = call.message_loc().unwrap();
    (l.start_offset(), l.end_offset())
}

/// NilAssertionHandleable (AssertNil / RefuteNil).
pub fn nil_assertion(cx: &mut Cx, node: &Node, ty: &str) {
    let call = node.as_call_node().unwrap();
    if call.receiver().is_some() {
        return;
    }
    let name = method_name(&call);
    let args = send_args(&call);
    let (eq, pred) = (format!("{ty}_equal"), format!("{ty}_predicate"));
    // (actual, index of the message argument, the inner `.nil?` to remove)
    let (actual, rest, remove) = if name == eq && args.len() >= 2 && args[0].as_nil_node().is_some() {
        (range(&args[1]), 2, None)
    } else if name == ty
        && let Some(inner) = args.first().and_then(as_send)
        && let Some(recv) = unary_send(&args[0], "nil?")
    {
        let dot = inner.call_operator_loc().map(|l| (l.start_offset(), l.end_offset()));
        (range(&recv), 1, Some((dot, selector(&inner))))
    } else if name == pred && args.len() >= 2 && is_sym(&args[1], "nil?") {
        (range(&args[0]), 2, None)
    } else {
        return;
    };
    let actual_src = cx.src.text[actual.0..actual.1].to_string();
    let mut shown = actual_src.clone();
    if let Some(m) = args.get(rest) {
        shown = format!("{shown}, {}", node_text(m));
    }
    let msg = format!("Prefer using `{ty}_nil({shown})`.");
    let sel = selector(&call);
    let both = (args.len() >= 2).then(|| first_and_second(&args));
    let (s, e) = send_range(&call);
    let ty_nil = format!("{ty}_nil");
    cx.add_fix(s, e, msg, |c| {
        c.replace(sel.0, sel.1, &ty_nil);
        match remove {
            None => {
                let r = both.unwrap();
                c.replace(r.0, r.1, &actual_src);
            }
            Some((dot, inner_sel)) => {
                if let Some(d) = dot {
                    c.remove(d.0, d.1);
                }
                c.remove(inner_sel.0, inner_sel.1);
            }
        }
    });
}

/// InDeltaMixin (AssertInDelta / RefuteInDelta): `(send nil? :<outer> $_ $_ $...)`.
pub fn in_delta(cx: &mut Cx, node: &Node, outer: &str, ty: &str) {
    let call = node.as_call_node().unwrap();
    let Some(args) = receiverless(&call, &[outer]) else { return };
    if args.len() < 2 {
        return;
    }
    let float = |n: &Node| helpers::literal_node::folded_kind(n) == kind::FLOAT_NODE;
    if !float(&args[0]) && !float(&args[1]) {
        return;
    }
    let (e, a) = (node_text(&args[0]), node_text(&args[1]));
    let good = match args.get(2) {
        Some(m) => format!("{ty}_in_delta({e}, {a}, 0.001, {})", node_text(m)),
        None => format!("{ty}_in_delta({e}, {a})"),
    };
    let msg = format!("Prefer using `{good}`.");
    let (s, end) = send_range(&call);
    cx.add_fix(s, end, msg, |c| c.replace(s, end, &good));
}

/// InstanceOfAssertionHandleable (AssertInstanceOf / RefuteInstanceOf).
pub fn instance_of(cx: &mut Cx, node: &Node, ty: &str) {
    let call = node.as_call_node().unwrap();
    if call.receiver().is_some() {
        return;
    }
    let name = method_name(&call);
    let args = send_args(&call);
    let eq = format!("{ty}_equal");
    // (first capture, second capture, message index)
    let (first, second, rest) = if name == ty && (1..=2).contains(&args.len()) {
        let Some(inner) = as_send(&args[0]) else { return };
        let inner_args = send_args(&inner);
        let Some(recv) = inner.receiver() else { return };
        if method_name(&inner) != "instance_of?" || inner_args.len() != 1 || !is_const(&inner_args[0]) {
            return;
        }
        (node_text(&recv), node_text(&inner_args[0]), 1)
    } else if name == eq && (2..=3).contains(&args.len()) && is_const(&args[0]) {
        let Some(recv) = unary_send(&args[1], "class") else { return };
        (node_text(&args[0]), node_text(&recv), 2)
    } else {
        return;
    };
    let required = if name == ty { format!("{second}, {first}") } else { format!("{first}, {second}") };
    let full = match args.get(rest) {
        Some(m) => format!("{required}, {}", node_text(m)),
        None => required.clone(),
    };
    let msg = format!("Prefer using `{ty}_instance_of({full})`.");
    let sel = selector(&call);
    let r = if name == ty { range(&args[0]) } else { first_and_second(&args) };
    let (s, e) = send_range(&call);
    let new_name = format!("{ty}_instance_of");
    cx.add_fix(s, e, msg, |c| {
        c.replace(sel.0, sel.1, &new_name);
        c.replace(r.0, r.1, &required);
    });
}

/// `(const _ :File)`
fn file_const(node: &Node) -> bool {
    match node {
        Node::ConstantReadNode { .. } => node.as_constant_read_node().unwrap().name().as_slice() == b"File",
        Node::ConstantPathNode { .. } => {
            node.as_constant_path_node().unwrap().name().is_some_and(|n| n.as_slice() == b"File")
        }
        _ => false,
    }
}

/// AssertPathExists / RefutePathExists.
pub fn path_exists(cx: &mut Cx, node: &Node, ty: &str) {
    let call = node.as_call_node().unwrap();
    let Some(args) = receiverless(&call, &[ty]) else { return };
    let Some(inner) = args.first().and_then(as_send) else { return };
    let inner_args = send_args(&inner);
    if !matches!(method_name(&inner), "exist?" | "exists?")
        || inner_args.len() != 1
        || !inner.receiver().is_some_and(|r| file_const(&r))
    {
        return;
    }
    let path = node_text(&inner_args[0]);
    let joined = match args.get(1) {
        Some(m) => format!("{path}, {}", node_text(m)),
        None => path.to_string(),
    };
    let good = if call.opening_loc().is_some_and(|o| o.as_slice() == b"(") {
        format!("{ty}_path_exists({joined})")
    } else {
        format!("{ty}_path_exists {joined}")
    };
    let msg = format!("Prefer using `{good}`.");
    let sel = selector(&call);
    let r = range(&args[0]);
    let (s, e) = send_range(&call);
    let new_name = format!("{ty}_path_exists");
    cx.add_fix(s, e, msg, |c| {
        c.replace(sel.0, sel.1, &new_name);
        c.replace(r.0, r.1, path);
    });
}

/// PredicateAssertionHandleable (AssertPredicate / RefutePredicate).
pub fn predicate(cx: &mut Cx, node: &Node, ty: &str) {
    let call = node.as_call_node().unwrap();
    if method_name(&call) != ty {
        return;
    }
    let args = send_args(&call);
    let Some(first) = args.first() else { return };
    if any_block_call(first).is_some() {
        return;
    }
    let Some(inner) = as_call(first) else { return };
    if !method_name(&inner).ends_with('?') || !send_args(&inner).is_empty() {
        return;
    }
    let new_args = format!("{}, :{}", receiver_source(&inner), method_name(&inner));
    let shown = match message_arg(cx, &args) {
        Some(m) => format!("{new_args}, {}", node_text(m)),
        None => new_args.clone(),
    };
    let msg = format!("Prefer using `{ty}_predicate({shown})`.");
    let sel = selector(&call);
    let r = range(first);
    let (s, e) = send_range(&call);
    let new_name = format!("{ty}_predicate");
    cx.add_fix(s, e, msg, |c| {
        c.replace(sel.0, sel.1, &new_name);
        c.replace(r.0, r.1, &new_args);
    });
}

/// AssertOperator / RefuteOperator.
pub fn operator(cx: &mut Cx, node: &Node, ty: &str) {
    let call = node.as_call_node().unwrap();
    if method_name(&call) != ty {
        return;
    }
    let args = send_args(&call);
    let Some(inner) = args.first().and_then(as_call) else { return };
    if !helpers::node_types::is_binary_operation(&inner) || method_name(&inner) == "[]" {
        return;
    }
    let (Some(lhs), Some(rhs)) = (inner.receiver(), send_args(&inner).into_iter().next()) else { return };
    let mut new_args = format!("{}, :{}, {}", node_text(&lhs), method_name(&inner), node_text(&rhs));
    if args.len() == 2 {
        new_args = format!("{new_args}, {}", node_text(&args[1]));
    }
    let msg = format!("Prefer using `{ty}_operator({new_args})`.");
    let sel = selector(&call);
    let r = all_arguments(&args);
    let (s, e) = send_range(&call);
    let new_name = format!("{ty}_operator");
    cx.add_fix(s, e, msg, |c| {
        c.replace(sel.0, sel.1, &new_name);
        c.replace(r.0, r.1, &new_args);
    });
}

/// AssertSame / RefuteSame.
pub fn same_cop(cx: &mut Cx, node: &Node, ty: &str) {
    let call = node.as_call_node().unwrap();
    if call.receiver().is_some() {
        return;
    }
    let name = method_name(&call);
    let args = send_args(&call);
    let sel = selector(&call);
    let (s, e) = send_range(&call);
    let new_name = format!("{ty}_same");
    if name == ty && (1..=2).contains(&args.len()) {
        let Some((exp, act)) = binop(&args[0], &["equal?"]) else { return };
        let pair = format!("{}, {}", node_text(&exp), node_text(&act));
        let shown = match args.get(1) {
            Some(m) => format!("{pair}, {}", node_text(m)),
            None => pair.clone(),
        };
        let r = range(&args[0]);
        cx.add_fix(s, e, format!("Prefer using `{ty}_same({shown})`."), |c| {
            c.replace(sel.0, sel.1, &new_name);
            c.replace(r.0, r.1, &pair);
        });
    } else if name == format!("{ty}_equal") && (2..=3).contains(&args.len()) {
        let (Some(exp), Some(act)) = (unary_send(&args[0], "object_id"), unary_send(&args[1], "object_id")) else {
            return;
        };
        let mut shown = format!("{}, {}", node_text(&exp), node_text(&act));
        if let Some(m) = args.get(2) {
            shown = format!("{shown}, {}", node_text(m));
        }
        let cut = |n: &Node| {
            let c = n.as_call_node().unwrap();
            let start = c.call_operator_loc().map_or(selector(&c).0, |d| d.start_offset());
            (start, selector(&c).1)
        };
        let (a, b) = (cut(&args[0]), cut(&args[1]));
        cx.add_fix(s, e, format!("Prefer using `{ty}_same({shown})`."), |c| {
            c.replace(sel.0, sel.1, &new_name);
            c.remove(a.0, a.1);
            c.remove(b.0, b.1);
        });
    }
}
