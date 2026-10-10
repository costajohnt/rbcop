use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Lint/AssignmentInCondition";
pub static COP: AssignmentInCondition = AssignmentInCondition;
pub struct AssignmentInCondition;

const MSG_ALLOWED: &str = "Use `==` if you meant to do a comparison or wrap the expression in parentheses to indicate \
                           you meant to assign in a condition.";
const MSG_NOT_ALLOWED: &str =
    "Use `==` if you meant to do a comparison or move the assignment up out of the condition.";

/// `assignment_method?`: ends with `=` and is not a comparison.
fn assignment_method(name: &[u8]) -> bool {
    name.ends_with(b"=") && !matches!(name, b"==" | b"===" | b"!=" | b"<=" | b">=")
}

/// `equals_asgn?` (lvasgn ivasgn cvasgn gvasgn casgn masgn) with its `loc.operator`.
fn equals_asgn<'pr>(node: &Node<'pr>) -> Option<ruby_prism::Location<'pr>> {
    Some(match kind::of(node) {
        kind::LOCAL_VARIABLE_WRITE_NODE => node.as_local_variable_write_node()?.operator_loc(),
        kind::INSTANCE_VARIABLE_WRITE_NODE => node.as_instance_variable_write_node()?.operator_loc(),
        kind::CLASS_VARIABLE_WRITE_NODE => node.as_class_variable_write_node()?.operator_loc(),
        kind::GLOBAL_VARIABLE_WRITE_NODE => node.as_global_variable_write_node()?.operator_loc(),
        kind::CONSTANT_WRITE_NODE => node.as_constant_write_node()?.operator_loc(),
        kind::CONSTANT_PATH_WRITE_NODE => node.as_constant_path_write_node()?.operator_loc(),
        kind::MULTI_WRITE_NODE => node.as_multi_write_node()?.operator_loc(),
        _ => return None,
    })
}

/// `setter_method?`: a call with `loc.operator` (`a.b = 1`, `a[i] = 1`).
fn setter_call(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| c.equal_loc().is_some())
}

/// Port of `traverse_node` over the condition. `stack` holds, per enclosing node, whether it is a
/// parser `begin` with more than one child (for `discarded_assignment?`).
struct Walk {
    allow_safe: bool,
    stack: Vec<bool>,
    flat_stmts: bool,
    /// (operator range, assignment node range)
    found: Vec<((usize, usize), (usize, usize))>,
}

impl Walk {
    fn parent_multi(&self) -> bool {
        self.stack.len() >= 2 && self.stack[self.stack.len() - 2]
    }

    fn report(&mut self, op: ruby_prism::Location, node: (usize, usize)) {
        if !self.parent_multi() {
            self.found.push(((op.start_offset(), op.end_offset()), node));
        }
    }

    /// A parser `begin` (parens / interpolation): true when its children are skipped.
    fn begin_skips(&self, body: Option<Node>) -> bool {
        let stmts: Vec<Node> = match body {
            None => return true, // empty_condition?
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
        };
        self.allow_safe && stmts.len() == 1 && (equals_asgn(&stmts[0]).is_some() || setter_call(&stmts[0]))
    }
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

impl<'pr> Visit<'pr> for Walk {
    fn visit_branch_node_enter(&mut self, _node: Node<'pr>) {
        self.stack.push(false);
    }
    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }
    fn visit_leaf_node_enter(&mut self, _node: Node<'pr>) {
        self.stack.push(false);
    }
    fn visit_leaf_node_leave(&mut self) {
        self.stack.pop();
    }

    fn visit_statements_node(&mut self, node: &ruby_prism::StatementsNode<'pr>) {
        let flat = std::mem::take(&mut self.flat_stmts);
        self.stack.push(!flat && node.body().iter().count() > 1);
        ruby_prism::visit_statements_node(self, node);
        self.stack.pop();
    }

    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        // kwbegin without rescue/ensure keeps its statements as direct children.
        self.flat_stmts = node.begin_keyword_loc().is_some()
            && node.statements().is_some()
            && node.rescue_clause().is_none()
            && node.ensure_clause().is_none()
            && node.else_clause().is_none();
        ruby_prism::visit_begin_node(self, node);
        self.flat_stmts = false;
    }

    fn visit_parentheses_node(&mut self, node: &ruby_prism::ParenthesesNode<'pr>) {
        if !self.begin_skips(node.body()) {
            ruby_prism::visit_parentheses_node(self, node);
        }
    }

    fn visit_embedded_statements_node(&mut self, node: &ruby_prism::EmbeddedStatementsNode<'pr>) {
        if !self.begin_skips(node.statements().map(|s| s.as_node())) {
            ruby_prism::visit_embedded_statements_node(self, node);
        }
    }

    fn visit_defined_node(&mut self, _node: &ruby_prism::DefinedNode<'pr>) {}
    fn visit_lambda_node(&mut self, _node: &ruby_prism::LambdaNode<'pr>) {}

    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        if node.block().and_then(|b| b.as_block_node()).is_none() {
            ruby_prism::visit_super_node(self, node);
        }
    }

    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        if node.block().is_none() {
            ruby_prism::visit_forwarding_super_node(self, node);
        }
    }

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if node.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        if !assignment_method(node.name().as_slice()) {
            return;
        }
        if let Some(op) = node.equal_loc() {
            self.report(op, send_range(node));
        }
        ruby_prism::visit_call_node(self, node);
    }

    // `match_with_lvasgn`: the regexp and the value, no `send`.
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        let call = node.call();
        if let Some(r) = call.receiver() {
            self.visit(&r);
        }
        if let Some(a) = call.arguments() {
            for x in a.arguments().iter() {
                self.visit(&x);
            }
        }
    }

    // `op_asgn` on a `send`: the send (`a.b` / `a[i]`) is not an assignment method, so only the
    // value is traversed.
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_call_and_write_node(&mut self, node: &ruby_prism::CallAndWriteNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_index_operator_write_node(&mut self, node: &ruby_prism::IndexOperatorWriteNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode<'pr>) {
        self.visit(&node.value());
    }
    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode<'pr>) {
        self.visit(&node.value());
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.report(node.operator_loc(), range(&node.as_node()));
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'pr>) {
        self.report(node.operator_loc(), range(&node.as_node()));
        ruby_prism::visit_instance_variable_write_node(self, node);
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'pr>) {
        self.report(node.operator_loc(), range(&node.as_node()));
        ruby_prism::visit_class_variable_write_node(self, node);
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'pr>) {
        self.report(node.operator_loc(), range(&node.as_node()));
        ruby_prism::visit_global_variable_write_node(self, node);
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'pr>) {
        self.report(node.operator_loc(), range(&node.as_node()));
        ruby_prism::visit_constant_write_node(self, node);
    }
    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode<'pr>) {
        self.report(node.operator_loc(), range(&node.as_node()));
        ruby_prism::visit_constant_path_write_node(self, node);
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        self.report(node.operator_loc(), range(&node.as_node()));
        ruby_prism::visit_multi_write_node(self, node);
    }
}

impl Cop for AssignmentInCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let condition = match kind::of(node) {
            kind::IF_NODE => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                node.as_if_node().unwrap().predicate()
            }
            kind::UNLESS_NODE => {
                if is_pattern_guard(node, cx.ancestors) {
                    return;
                }
                node.as_unless_node().unwrap().predicate()
            }
            kind::WHILE_NODE => {
                let w = node.as_while_node().unwrap();
                if w.is_begin_modifier() {
                    return;
                }
                w.predicate()
            }
            _ => {
                let u = node.as_until_node().unwrap();
                if u.is_begin_modifier() {
                    return;
                }
                u.predicate()
            }
        };
        let allow_safe = cx.cfg.bool("AllowSafeAssignment") == Some(true);
        let mut walk = Walk { allow_safe, stack: vec![false], flat_stmts: false, found: Vec::new() };
        walk.visit(&condition);
        let msg = if allow_safe { MSG_ALLOWED } else { MSG_NOT_ALLOWED };
        for ((s, e), (ns, ne)) in walk.found {
            cx.add_fix(s, e, msg, |c| {
                if allow_safe {
                    c.wrap(ns, ne, "(", ")");
                }
            });
        }
    }
}
