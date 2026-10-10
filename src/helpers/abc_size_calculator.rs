//! Metrics::Utils::AbcSizeCalculator with RepeatedAttributeDiscount (Metrics/AbcSize).
//!
//! The calculator visits the parser tree depth-last (children before the node); this walks the
//! Prism tree in the same order, processing the parser nodes Prism folds into other nodes (the
//! `send` of an `a.b ||= c`, a `lambda` send, the `lvasgn` of `x += 1`) where parser has them.

use crate::helpers::method_complexity::{CsendDiscount, iterating_method, lvar_name, plain_block};
use crate::node_kind as kind;
use ruby_prism::{CallNode, Node, Visit};
use std::collections::HashSet;

/// `[abc size rounded to 2 places, "<A, B, C>"]`
pub fn calculate(body: &Node, discount_repeated_attributes: bool) -> (f64, String) {
    let mut v = Abc {
        a: 0,
        b: 0,
        c: 0,
        csend: CsendDiscount::default(),
        attrs: discount_repeated_attributes.then(|| HashSet::from([vec!["self".to_string()]])),
        pattern: 0,
        guard: None,
    };
    v.visit(body);
    let size = ((v.a * v.a + v.b * v.b + v.c * v.c) as f64).sqrt();
    (round2(size), format!("<{}, {}, {}>", v.a, v.b, v.c))
}

/// Ruby `Float#round(2)` (round_half_up with its correction step).
fn round2(x: f64) -> f64 {
    let s = 100.0;
    let mut f = (x * s).round();
    if x > 0.0 && (f + 0.5) / s <= x {
        f += 1.0;
    }
    f / s
}

struct Abc {
    a: i64,
    b: i64,
    c: i64,
    csend: CsendDiscount,
    /// RepeatedAttributeDiscount's known attributes as paths from a root (`self`, `lvar:x`, ...).
    attrs: Option<HashSet<Vec<String>>>,
    pattern: usize,
    guard: Option<(usize, usize)>,
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

fn name(c: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(c.as_slice()).into_owned()
}

/// `capturing_variable?`
fn capturing(name: &[u8]) -> bool {
    !name.starts_with(b"_")
}

/// `/re/ =~ x` with a static regexp is parser `match_with_lvasgn`, not a send.
fn match_with_lvasgn(call: &CallNode) -> bool {
    call.name().as_slice() == b"=~" && call.receiver().is_some_and(|r| r.as_regular_expression_node().is_some())
}

/// A send with no arguments (`attribute_call?` on the send itself).
fn no_args(call: &CallNode) -> bool {
    call.arguments().is_none() && !call.block().is_some_and(|b| b.as_block_argument_node().is_some())
}

/// `root_node?` key: `nil` and `self` share one.
fn root_key(node: Option<&Node>) -> Option<String> {
    let Some(node) = node else { return Some("self".into()) };
    Some(match kind::of(node) {
        kind::SELF_NODE => "self".into(),
        kind::LOCAL_VARIABLE_READ_NODE | kind::IT_LOCAL_VARIABLE_READ_NODE => {
            format!("lvar:{}", String::from_utf8_lossy(&lvar_name(node)?))
        }
        kind::INSTANCE_VARIABLE_READ_NODE => format!("ivar:{}", name(&node.as_instance_variable_read_node()?.name())),
        kind::CLASS_VARIABLE_READ_NODE => format!("cvar:{}", name(&node.as_class_variable_read_node()?.name())),
        kind::GLOBAL_VARIABLE_READ_NODE => format!("gvar:{}", name(&node.as_global_variable_read_node()?.name())),
        kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE => {
            format!("const:{}", crate::ast::node_text(node).split_whitespace().collect::<String>())
        }
        _ => return None,
    })
}

/// `find_attributes` path of a receiver: attribute calls down to a root node.
fn path(node: Option<&Node>) -> Option<Vec<String>> {
    if let Some(call) = node.and_then(|n| n.as_call_node())
        && !match_with_lvasgn(&call)
        && no_args(&call)
        && !call.block().is_some_and(|b| b.as_block_node().is_some())
    {
        let mut p = path(call.receiver().as_ref())?;
        p.push(name(&call.name()));
        return Some(p);
    }
    root_key(node).map(|k| vec![k])
}

/// MethodDispatchNode (send csend super zsuper yield defined?) that is not a setter: a value an
/// op-assign "miscounts".
fn dispatch_value(node: &Node) -> bool {
    match kind::of(node) {
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            !match_with_lvasgn(&c)
                && c.equal_loc().is_none()
                && !c.block().is_some_and(|b| b.as_block_node().is_some())
        }
        kind::SUPER_NODE => !node.as_super_node().unwrap().block().is_some_and(|b| b.as_block_node().is_some()),
        kind::FORWARDING_SUPER_NODE => node.as_forwarding_super_node().unwrap().block().is_none(),
        kind::YIELD_NODE | kind::DEFINED_NODE => true,
        _ => false,
    }
}

/// Send targets of a multiple assignment (`MlhsNode#assignments` that are sends).
fn send_targets(targets: &mut dyn Iterator<Item = Node>) -> i64 {
    let mut n = 0;
    for t in targets {
        n += match kind::of(&t) {
            kind::CALL_TARGET_NODE | kind::INDEX_TARGET_NODE => 1,
            kind::MULTI_TARGET_NODE => {
                let m = t.as_multi_target_node().unwrap();
                send_targets(&mut m.lefts().iter().chain(crate::ast::parser_rest(m.rest())).chain(m.rights().iter()))
            }
            kind::SPLAT_NODE => t.as_splat_node().unwrap().expression().map_or(0, |e| {
                matches!(kind::of(&e), kind::CALL_TARGET_NODE | kind::INDEX_TARGET_NODE) as i64
            }),
            _ => 0,
        };
    }
    n
}

impl Abc {
    /// RepeatedAttributeDiscount#discount_repeated_attribute? (registers the attribute).
    fn repeated_attribute(&mut self, receiver: Option<&Node>, method: &str) -> bool {
        let Some(attrs) = self.attrs.as_mut() else { return false };
        let Some(mut p) = path(receiver) else { return false };
        p.push(method.to_string());
        let mut repeated = true;
        for i in 1..=p.len() {
            if !attrs.contains(&p[..i]) {
                attrs.insert(p[..i].to_vec());
                repeated = false;
            }
        }
        repeated
    }

    /// `update_repeated_attribute` for a variable assignment: forget the variable's attributes.
    fn clear_var(&mut self, key: String) {
        if let Some(attrs) = self.attrs.as_mut() {
            attrs.retain(|p| p.len() < 2 || p[0] != key);
        }
    }

    /// `update_repeated_attribute` for `recv.method = ` / `recv.method ||= `: forget that attribute.
    fn forget(&mut self, receiver: Option<&Node>, method: &str) {
        let Some(attrs) = self.attrs.as_mut() else { return };
        let Some(mut p) = path(receiver) else { return };
        if !attrs.contains(&p) {
            return;
        }
        p.push(method.to_string());
        attrs.retain(|q| !q.starts_with(&p));
    }

    /// A parser `send` / `csend` node: assignment (setter) and branch / condition.
    fn send(&mut self, receiver: Option<&Node>, method: &str, args: bool, csend: bool, setter: bool, at: usize) {
        if setter {
            self.forget(receiver, &method[..method.len() - 1]);
            self.a += 1;
        }
        if !args && self.repeated_attribute(receiver, method) {
            return;
        }
        if matches!(method, "==" | "===" | "!=" | "<=" | ">=" | ">" | "<") {
            self.c += 1;
        } else {
            self.b += 1;
            if csend && !self.csend.discount_receiver(receiver, at) {
                self.c += 1;
            }
        }
    }

    fn lvasgn(&mut self, name: &[u8]) {
        self.clear_var(format!("lvar:{}", String::from_utf8_lossy(name)));
        self.csend.reset(name);
        if capturing(name) {
            self.a += 1;
        }
    }

    fn var_asgn(&mut self, prefix: &str, name: &ruby_prism::ConstantId) {
        self.clear_var(format!("{prefix}:{}", self::name(name)));
        self.a += 1;
    }

    fn arg(&mut self, name: Option<ruby_prism::ConstantId>) {
        if name.is_some_and(|n| capturing(n.as_slice())) {
            self.a += 1;
        }
    }

    /// op_asgn / or_asgn / and_asgn after their children: `lhs` is the shorthand target's
    /// (receiver, method) when it is a send or casgn, `lhs_send` whether it is a send.
    fn shorthand(&mut self, lhs: Option<(Option<&Node>, String)>, lhs_send: bool, value: &Node, condition: bool) {
        if let Some((recv, method)) = lhs {
            self.forget(recv, &method);
        }
        self.a += lhs_send as i64 + dispatch_value(value) as i64;
        if condition {
            self.c += 1;
        }
    }

    fn visit_call_parts(&mut self, receiver: Option<Node>, call: &CallNode) {
        if let Some(r) = receiver {
            self.visit(&r);
        }
        if let Some(a) = call.arguments() {
            self.visit_arguments_node(&a);
        }
    }
}

impl<'pr> Visit<'pr> for Abc {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        // `- 1` is one literal for RuboCop, not a `-@` send.
        if crate::helpers::literal_node::folded_numeric(&node.as_node()).is_some() {
            return;
        }
        if match_with_lvasgn(node) {
            return self.visit_call_parts(node.receiver(), node);
        }
        self.visit_call_parts(node.receiver(), node);
        if let Some(b) = node.block().filter(|b| b.as_block_argument_node().is_some()) {
            self.visit(&b);
            // block_pass: a condition when the call iterates
            if iterating_method(node.name().as_slice()) {
                self.c += 1;
            }
        }
        let method = name(&node.name());
        let csend = crate::ast::is_csend(node);
        self.send(
            node.receiver().as_ref(),
            &method,
            !no_args(node),
            csend,
            node.equal_loc().is_some(),
            node.location().start_offset(),
        );
        if let Some(b) = node.block().filter(|b| b.as_block_node().is_some()) {
            self.visit(&b);
            if plain_block(node).is_some() && iterating_method(node.name().as_slice()) {
                self.c += 1;
            }
        }
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.send(None, "lambda", false, false, false, node.location().start_offset());
        ruby_prism::visit_lambda_node(self, node);
    }
    fn visit_yield_node(&mut self, node: &ruby_prism::YieldNode<'pr>) {
        ruby_prism::visit_yield_node(self, node);
        self.b += 1;
    }
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        let call = node.call();
        self.visit_call_parts(call.receiver(), &call);
    }

    // Conditions.
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        ruby_prism::visit_if_node(self, node);
        if self.guard != Some(range(&node.as_node())) {
            self.c += 1;
            let else_kw = node
                .subsequent()
                .and_then(|s| s.as_else_node())
                .is_some_and(|e| e.else_keyword_loc().as_slice() == b"else");
            self.c += else_kw as i64;
        }
    }
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        ruby_prism::visit_unless_node(self, node);
        if self.guard != Some(range(&node.as_node())) {
            self.c += 1 + node.else_clause().is_some() as i64;
        }
    }
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'pr>) {
        ruby_prism::visit_while_node(self, node);
        self.c += !node.is_begin_modifier() as i64;
    }
    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'pr>) {
        ruby_prism::visit_until_node(self, node);
        self.c += !node.is_begin_modifier() as i64;
    }
    fn visit_for_node(&mut self, node: &ruby_prism::ForNode<'pr>) {
        ruby_prism::visit_for_node(self, node);
        self.a += 1;
        self.c += 1;
    }
    fn visit_when_node(&mut self, node: &ruby_prism::WhenNode<'pr>) {
        ruby_prism::visit_when_node(self, node);
        self.c += 1;
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        let pattern = node.pattern();
        let saved = self.guard;
        if matches!(kind::of(&pattern), kind::IF_NODE | kind::UNLESS_NODE) {
            self.guard = Some(range(&pattern));
        }
        self.pattern += 1;
        self.visit(&pattern);
        self.pattern -= 1;
        self.guard = saved;
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
        self.c += 1;
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
        self.pattern += 1;
        self.visit(&node.pattern());
        self.pattern -= 1;
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
        self.pattern += 1;
        self.visit(&node.pattern());
        self.pattern -= 1;
    }
    fn visit_and_node(&mut self, node: &ruby_prism::AndNode<'pr>) {
        ruby_prism::visit_and_node(self, node);
        self.c += 1;
    }
    fn visit_or_node(&mut self, node: &ruby_prism::OrNode<'pr>) {
        ruby_prism::visit_or_node(self, node);
        self.c += 1;
    }
    fn visit_begin_node(&mut self, node: &ruby_prism::BeginNode<'pr>) {
        ruby_prism::visit_begin_node(self, node);
        self.c += node.rescue_clause().is_some() as i64;
    }
    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode<'pr>) {
        ruby_prism::visit_rescue_modifier_node(self, node);
        self.c += 1;
    }

    // Assignments.
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        ruby_prism::visit_local_variable_write_node(self, node);
        self.lvasgn(node.name().as_slice());
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        if self.pattern == 0 {
            self.lvasgn(node.name().as_slice());
        }
    }
    fn visit_instance_variable_write_node(&mut self, node: &ruby_prism::InstanceVariableWriteNode<'pr>) {
        ruby_prism::visit_instance_variable_write_node(self, node);
        self.var_asgn("ivar", &node.name());
    }
    fn visit_instance_variable_target_node(&mut self, node: &ruby_prism::InstanceVariableTargetNode<'pr>) {
        self.var_asgn("ivar", &node.name());
    }
    fn visit_class_variable_write_node(&mut self, node: &ruby_prism::ClassVariableWriteNode<'pr>) {
        ruby_prism::visit_class_variable_write_node(self, node);
        self.var_asgn("cvar", &node.name());
    }
    fn visit_class_variable_target_node(&mut self, node: &ruby_prism::ClassVariableTargetNode<'pr>) {
        self.var_asgn("cvar", &node.name());
    }
    fn visit_global_variable_write_node(&mut self, node: &ruby_prism::GlobalVariableWriteNode<'pr>) {
        ruby_prism::visit_global_variable_write_node(self, node);
        self.var_asgn("gvar", &node.name());
    }
    fn visit_global_variable_target_node(&mut self, node: &ruby_prism::GlobalVariableTargetNode<'pr>) {
        self.var_asgn("gvar", &node.name());
    }
    fn visit_constant_write_node(&mut self, node: &ruby_prism::ConstantWriteNode<'pr>) {
        ruby_prism::visit_constant_write_node(self, node);
        self.a += 1;
    }
    fn visit_constant_path_write_node(&mut self, node: &ruby_prism::ConstantPathWriteNode<'pr>) {
        ruby_prism::visit_constant_path_write_node(self, node);
        self.a += 1;
    }
    fn visit_constant_target_node(&mut self, _node: &ruby_prism::ConstantTargetNode<'pr>) {
        self.a += 1;
    }
    fn visit_constant_path_target_node(&mut self, node: &ruby_prism::ConstantPathTargetNode<'pr>) {
        ruby_prism::visit_constant_path_target_node(self, node);
        self.a += 1;
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        ruby_prism::visit_multi_write_node(self, node);
        self.a += send_targets(&mut node.lefts().iter().chain(crate::ast::parser_rest(node.rest())).chain(node.rights().iter()));
    }
    fn visit_call_target_node(&mut self, node: &ruby_prism::CallTargetNode<'pr>) {
        let recv = node.receiver();
        self.visit(&recv);
        let csend = node.call_operator_loc().as_slice() == b"&.";
        self.send(Some(&recv), &name(&node.name()), false, csend, false, node.location().start_offset());
    }
    fn visit_index_target_node(&mut self, node: &ruby_prism::IndexTargetNode<'pr>) {
        ruby_prism::visit_index_target_node(self, node);
        self.send(Some(&node.receiver()), "[]=", true, false, false, node.location().start_offset());
    }

    // Shorthand assignments: the target, the value, then the op_asgn / or_asgn / and_asgn.
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.lvasgn(node.name().as_slice());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), false);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.lvasgn(node.name().as_slice());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.lvasgn(node.name().as_slice());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_instance_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::InstanceVariableOperatorWriteNode<'pr>,
    ) {
        self.var_asgn("ivar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), false);
    }
    fn visit_instance_variable_or_write_node(&mut self, node: &ruby_prism::InstanceVariableOrWriteNode<'pr>) {
        self.var_asgn("ivar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_instance_variable_and_write_node(&mut self, node: &ruby_prism::InstanceVariableAndWriteNode<'pr>) {
        self.var_asgn("ivar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_class_variable_operator_write_node(&mut self, node: &ruby_prism::ClassVariableOperatorWriteNode<'pr>) {
        self.var_asgn("cvar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), false);
    }
    fn visit_class_variable_or_write_node(&mut self, node: &ruby_prism::ClassVariableOrWriteNode<'pr>) {
        self.var_asgn("cvar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_class_variable_and_write_node(&mut self, node: &ruby_prism::ClassVariableAndWriteNode<'pr>) {
        self.var_asgn("cvar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_global_variable_operator_write_node(
        &mut self,
        node: &ruby_prism::GlobalVariableOperatorWriteNode<'pr>,
    ) {
        self.var_asgn("gvar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), false);
    }
    fn visit_global_variable_or_write_node(&mut self, node: &ruby_prism::GlobalVariableOrWriteNode<'pr>) {
        self.var_asgn("gvar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_global_variable_and_write_node(&mut self, node: &ruby_prism::GlobalVariableAndWriteNode<'pr>) {
        self.var_asgn("gvar", &node.name());
        self.visit(&node.value());
        self.shorthand(None, false, &node.value(), true);
    }
    fn visit_constant_operator_write_node(&mut self, node: &ruby_prism::ConstantOperatorWriteNode<'pr>) {
        self.a += 1;
        self.visit(&node.value());
        self.shorthand(Some((None, name(&node.name()))), false, &node.value(), false);
    }
    fn visit_constant_or_write_node(&mut self, node: &ruby_prism::ConstantOrWriteNode<'pr>) {
        self.a += 1;
        self.visit(&node.value());
        self.shorthand(Some((None, name(&node.name()))), false, &node.value(), true);
    }
    fn visit_constant_and_write_node(&mut self, node: &ruby_prism::ConstantAndWriteNode<'pr>) {
        self.a += 1;
        self.visit(&node.value());
        self.shorthand(Some((None, name(&node.name()))), false, &node.value(), true);
    }
    fn visit_constant_path_operator_write_node(&mut self, node: &ruby_prism::ConstantPathOperatorWriteNode<'pr>) {
        let t = node.target();
        ruby_prism::visit_constant_path_node(self, &t);
        self.a += 1;
        self.visit(&node.value());
        let method = t.name().map(|n| name(&n)).unwrap_or_default();
        self.shorthand(Some((t.parent().as_ref(), method)), false, &node.value(), false);
    }
    fn visit_constant_path_or_write_node(&mut self, node: &ruby_prism::ConstantPathOrWriteNode<'pr>) {
        let t = node.target();
        ruby_prism::visit_constant_path_node(self, &t);
        self.a += 1;
        self.visit(&node.value());
        let method = t.name().map(|n| name(&n)).unwrap_or_default();
        self.shorthand(Some((t.parent().as_ref(), method)), false, &node.value(), true);
    }
    fn visit_constant_path_and_write_node(&mut self, node: &ruby_prism::ConstantPathAndWriteNode<'pr>) {
        let t = node.target();
        ruby_prism::visit_constant_path_node(self, &t);
        self.a += 1;
        self.visit(&node.value());
        let method = t.name().map(|n| name(&n)).unwrap_or_default();
        self.shorthand(Some((t.parent().as_ref(), method)), false, &node.value(), true);
    }
    fn visit_call_operator_write_node(&mut self, node: &ruby_prism::CallOperatorWriteNode<'pr>) {
        let recv = node.receiver();
        if let Some(r) = &recv {
            self.visit(r);
        }
        let method = name(&node.read_name());
        let csend = node.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.");
        self.send(recv.as_ref(), &method, false, csend, false, node.location().start_offset());
        self.visit(&node.value());
        self.shorthand(Some((recv.as_ref(), method)), true, &node.value(), false);
    }
    fn visit_call_or_write_node(&mut self, node: &ruby_prism::CallOrWriteNode<'pr>) {
        let recv = node.receiver();
        if let Some(r) = &recv {
            self.visit(r);
        }
        let method = name(&node.read_name());
        let csend = node.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.");
        self.send(recv.as_ref(), &method, false, csend, false, node.location().start_offset());
        self.visit(&node.value());
        self.shorthand(Some((recv.as_ref(), method)), true, &node.value(), true);
    }
    fn visit_call_and_write_node(&mut self, node: &ruby_prism::CallAndWriteNode<'pr>) {
        let recv = node.receiver();
        if let Some(r) = &recv {
            self.visit(r);
        }
        let method = name(&node.read_name());
        let csend = node.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.");
        self.send(recv.as_ref(), &method, false, csend, false, node.location().start_offset());
        self.visit(&node.value());
        self.shorthand(Some((recv.as_ref(), method)), true, &node.value(), true);
    }
    fn visit_index_operator_write_node(&mut self, node: &ruby_prism::IndexOperatorWriteNode<'pr>) {
        self.index_write(node.receiver(), node.arguments(), node.block(), node.call_operator_loc(), &node.value(), false, node.location().start_offset());
    }
    fn visit_index_or_write_node(&mut self, node: &ruby_prism::IndexOrWriteNode<'pr>) {
        self.index_write(node.receiver(), node.arguments(), node.block(), node.call_operator_loc(), &node.value(), true, node.location().start_offset());
    }
    fn visit_index_and_write_node(&mut self, node: &ruby_prism::IndexAndWriteNode<'pr>) {
        self.index_write(node.receiver(), node.arguments(), node.block(), node.call_operator_loc(), &node.value(), true, node.location().start_offset());
    }

    // Arguments.
    fn visit_required_parameter_node(&mut self, node: &ruby_prism::RequiredParameterNode<'pr>) {
        self.arg(Some(node.name()));
    }
    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode<'pr>) {
        ruby_prism::visit_optional_parameter_node(self, node);
        self.arg(Some(node.name()));
    }
    fn visit_rest_parameter_node(&mut self, node: &ruby_prism::RestParameterNode<'pr>) {
        self.arg(node.name());
    }
    fn visit_required_keyword_parameter_node(&mut self, node: &ruby_prism::RequiredKeywordParameterNode<'pr>) {
        self.arg(Some(node.name()));
    }
    fn visit_optional_keyword_parameter_node(&mut self, node: &ruby_prism::OptionalKeywordParameterNode<'pr>) {
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
        self.arg(Some(node.name()));
    }
    fn visit_keyword_rest_parameter_node(&mut self, node: &ruby_prism::KeywordRestParameterNode<'pr>) {
        self.arg(node.name());
    }
    fn visit_block_parameter_node(&mut self, node: &ruby_prism::BlockParameterNode<'pr>) {
        self.arg(node.name());
    }
    fn visit_block_local_variable_node(&mut self, node: &ruby_prism::BlockLocalVariableNode<'pr>) {
        self.arg(Some(node.name()));
    }
}

impl Abc {
    #[allow(clippy::too_many_arguments)]
    fn index_write<'pr>(
        &mut self,
        recv: Option<Node<'pr>>,
        args: Option<ruby_prism::ArgumentsNode<'pr>>,
        block: Option<ruby_prism::BlockArgumentNode<'pr>>,
        op: Option<ruby_prism::Location<'pr>>,
        value: &Node<'pr>,
        condition: bool,
        at: usize,
    ) {
        if let Some(r) = &recv {
            self.visit(r);
        }
        if let Some(a) = &args {
            self.visit_arguments_node(a);
        }
        if let Some(b) = &block {
            self.visit_block_argument_node(b);
        }
        let has_args = args.is_some() || block.is_some();
        let csend = op.is_some_and(|l| l.as_slice() == b"&.");
        self.send(recv.as_ref(), "[]", has_args, csend, false, at);
        self.visit(value);
        self.shorthand(Some((recv.as_ref(), "[]".to_string())), true, value, condition);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round2() {
        assert_eq!(super::round2(2.0f64.sqrt()), 1.41);
        assert_eq!(super::round2(17.0), 17.0);
    }
}
