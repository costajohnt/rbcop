//! Style/RedundantSelf keeps `@local_variables_scopes`, an identity hash from parser nodes to name
//! arrays, where `add_scope` (on def and block) points every descendant at one shared array. A
//! node's array is therefore that of its innermost def (a fresh one per def), or, inside blocks
//! that are not in a def, the outermost such block's own entry, or else the node's own entry.
//! This port walks the tree in parser order with a stack of parser-shaped frames and resolves
//! each write and read to one of those arrays.

use crate::prelude::*;
use ruby_prism::{CallNode, Visit};
use std::collections::{HashMap, HashSet};

pub const NAME: &str = "Style/RedundantSelf";
pub static COP: RedundantSelf = RedundantSelf;
pub struct RedundantSelf;

/// `Kernel.methods(false)` in the RuboCop process.
const KERNEL_METHODS: &[&str] = &[
    "Array", "Complex", "Float", "Hash", "Integer", "Pathname", "Rational", "String", "URI", "__callee__", "__dir__",
    "__method__", "`", "abort", "at_exit", "autoload", "autoload?", "binding", "block_given?", "caller",
    "caller_locations", "catch", "eval", "exec", "exit", "exit!", "fail", "fork", "format", "gets",
    "global_variables", "iterator?", "lambda", "load", "local_variables", "loop", "open", "p", "print", "printf",
    "proc", "putc", "puts", "raise", "rand", "readline", "readlines", "require", "require_relative", "select",
    "set_trace_func", "sleep", "spawn", "sprintf", "srand", "syscall", "system", "test", "throw", "trace_var", "trap",
    "untrace_var", "warn",
];

const KEYWORDS: &[&str] = &[
    "alias", "and", "begin", "break", "case", "class", "def", "defined?", "do", "else", "elsif", "end", "ensure",
    "false", "for", "if", "in", "module", "next", "nil", "not", "or", "redo", "rescue", "retry", "return", "self",
    "super", "then", "true", "undef", "unless", "until", "when", "while", "yield", "__FILE__", "__LINE__",
    "__ENCODING__",
];

/// Parser node identity: range plus a type tag (a block call is tagged apart from its send).
type Key = (usize, usize, usize);
const BLOCK_TAG: usize = usize::MAX;

/// One of `@local_variables_scopes`' arrays.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Arr {
    /// The array `add_scope` creates for a def's descendants.
    Def(Key),
    /// A node's own entry.
    Own(Key),
}

#[derive(Clone, Copy, PartialEq)]
enum Fk {
    Def,
    /// `block` (incl. lambda literals): `plain` is false for numblock/itblock; `bare` when it has no
    /// arguments and no `||`.
    Block { plain: bool, bare: bool },
    Other,
    /// Prism-only wrapper.
    Transparent,
}

struct Frame {
    kind: Fk,
    /// The array this node's own key resolves to.
    res: Arr,
    /// What a descendant whose innermost def/block is at or above this frame resolves to.
    inner: Option<Arr>,
}

fn loc(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

fn has_block(call: &CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_node().is_some())
}

fn key_of(node: &Node) -> Key {
    let (s, e) = loc(node);
    let tag = match node.as_call_node() {
        Some(c) if has_block(&c) => BLOCK_TAG,
        _ if node.as_lambda_node().is_some() => BLOCK_TAG,
        _ => kind::of(node),
    };
    (s, e, tag)
}

fn name(id: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(id.as_slice()).into_owned()
}

struct Walker {
    frames: Vec<Frame>,
    names: HashMap<Arr, HashSet<String>>,
    /// A pattern guard (`in pat if cond`) is no `if` in parser.
    guards: HashSet<(usize, usize)>,
    /// (self start, self end, dot start, dot end)
    offenses: Vec<(usize, usize, usize, usize)>,
}

impl Walker {
    fn inner(&self) -> Option<Arr> {
        self.frames.last().and_then(|f| f.inner)
    }

    fn push(&mut self, key: Key, kind: Fk) {
        let parent = self.inner();
        let res = parent.unwrap_or(Arr::Own(key));
        let inner = match kind {
            Fk::Def => Some(Arr::Def(key)),
            Fk::Block { .. } => Some(res),
            Fk::Other | Fk::Transparent => parent,
        };
        self.frames.push(Frame { kind, res, inner });
    }

    /// `@local_variables_scopes[key] << name` for a key that is a child of the current node.
    fn write(&mut self, key: Key, name: String) {
        let arr = self.inner().unwrap_or(Arr::Own(key));
        self.names.entry(arr).or_default().insert(name);
    }

    /// `@local_variables_scopes[node] << name` for the current node.
    fn write_self(&mut self, name: String) {
        let arr = self.frames.last().unwrap().res;
        self.names.entry(arr).or_default().insert(name);
    }

    /// add_lhs_to_local_variables_scopes(rhs, lhs)
    fn add_lhs(&mut self, rhs: &Node, lhs: &str) {
        if let Some(call) = rhs.as_call_node().filter(|c| !is_csend(c) && !has_block(c)) {
            let mut args = call_args(&call);
            if let Some(b) = call.block() {
                args.push(b);
            }
            if !args.is_empty() {
                for a in &args {
                    self.write(key_of(a), lhs.to_string());
                }
                return;
            }
        }
        self.write(key_of(rhs), lhs.to_string());
    }

    /// on_args: every argument name of a def/block/lambda goes to that scope's array.
    fn add_params(&mut self, params: Option<Node>) {
        let Some(params) = params else { return };
        let mut out = Vec::new();
        if let Some(bp) = params.as_block_parameters_node() {
            if let Some(p) = bp.parameters() {
                param_names(&p.as_node(), &mut out);
            }
            for l in bp.locals().iter() {
                if let Some(l) = l.as_block_local_variable_node() {
                    out.push(name(&l.name()));
                }
            }
        } else if params.as_parameters_node().is_some() {
            param_names(&params, &mut out);
        }
        let arr = self.inner();
        for n in out {
            if let Some(arr) = arr {
                self.names.entry(arr).or_default().insert(n);
            }
        }
    }

    fn check_send(&mut self, call: &CallNode) {
        let Some(recv) = call.receiver() else { return };
        if recv.as_self_node().is_none() || is_csend(call) {
            return;
        }
        let m = method_name(call);
        if helpers::node_types::is_operator_method(m)
            || KEYWORDS.contains(&m)
            || m.starts_with(|c: char| c.is_ascii_uppercase())
            || call.equal_loc().is_some()
            || call.message_loc().is_none()
        {
            return;
        }
        if KERNEL_METHODS.contains(&m) {
            return;
        }
        let allowed = self
            .frames
            .iter()
            .filter(|f| f.kind != Fk::Transparent)
            .any(|f| self.names.get(&f.res).is_some_and(|n| n.contains(m)));
        if allowed {
            return;
        }
        // it_method_in_block?
        if m == "it" {
            // The current frame is the send itself.
            let block = self.frames[..self.frames.len() - 1]
                .iter()
                .rev()
                .find_map(|f| match f.kind {
                    Fk::Block { plain: true, bare } => Some(bare),
                    _ => None,
                });
            if block == Some(true) && call.arguments().is_none() && call.block().is_none() {
                return;
            }
        }
        let Some(dot) = call.call_operator_loc() else { return };
        let (s, e) = loc(&recv);
        self.offenses.push((s, e, dot.start_offset(), dot.end_offset()));
    }

    /// on_if / on_while / on_until: every lvasgn/masgn below goes to the condition.
    fn on_conditional(&mut self, node: &Node, cond: &Node) {
        let mut c = LvasgnNames { names: Vec::new(), pattern_depth: 0 };
        c.visit(node);
        for n in c.names {
            self.add_lhs(cond, &n);
        }
    }
}

/// Names of parser `arg`-like nodes in a parameter list (mlhs recursed).
fn param_names(node: &Node, out: &mut Vec<String>) {
    match kind::of(node) {
        kind::PARAMETERS_NODE => {
            let p = node.as_parameters_node().unwrap();
            for n in p.requireds().iter().chain(p.optionals().iter()) {
                param_names(&n, out);
            }
            if let Some(r) = crate::ast::parser_rest(p.rest()) {
                param_names(&r, out);
            }
            for n in p.posts().iter().chain(p.keywords().iter()) {
                param_names(&n, out);
            }
            if let Some(r) = p.keyword_rest() {
                param_names(&r, out);
            }
            if let Some(n) = p.block().and_then(|b| b.name()) {
                out.push(name(&n));
            }
        }
        kind::MULTI_TARGET_NODE => {
            let m = node.as_multi_target_node().unwrap();
            for n in m.lefts().iter() {
                param_names(&n, out);
            }
            if let Some(r) = crate::ast::parser_rest(m.rest()) {
                param_names(&r, out);
            }
            for n in m.rights().iter() {
                param_names(&n, out);
            }
        }
        kind::SPLAT_NODE => {
            if let Some(e) = node.as_splat_node().unwrap().expression() {
                param_names(&e, out);
            }
        }
        kind::REQUIRED_PARAMETER_NODE => out.push(name(&node.as_required_parameter_node().unwrap().name())),
        kind::OPTIONAL_PARAMETER_NODE => out.push(name(&node.as_optional_parameter_node().unwrap().name())),
        kind::REQUIRED_KEYWORD_PARAMETER_NODE => {
            out.push(name(&node.as_required_keyword_parameter_node().unwrap().name()))
        }
        kind::OPTIONAL_KEYWORD_PARAMETER_NODE => {
            out.push(name(&node.as_optional_keyword_parameter_node().unwrap().name()))
        }
        kind::REST_PARAMETER_NODE => {
            if let Some(n) = node.as_rest_parameter_node().unwrap().name() {
                out.push(name(&n));
            }
        }
        kind::KEYWORD_REST_PARAMETER_NODE => {
            if let Some(n) = node.as_keyword_rest_parameter_node().unwrap().name() {
                out.push(name(&n));
            }
        }
        _ => {}
    }
}

/// Names of parser `lvasgn` nodes in a subtree (not pattern variables or named captures).
struct LvasgnNames {
    names: Vec<String>,
    pattern_depth: usize,
}

impl<'pr> Visit<'pr> for LvasgnNames {
    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.names.push(name(&node.name()));
        ruby_prism::visit_local_variable_write_node(self, node);
    }
    fn visit_local_variable_operator_write_node(&mut self, node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>) {
        self.names.push(name(&node.name()));
        ruby_prism::visit_local_variable_operator_write_node(self, node);
    }
    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.names.push(name(&node.name()));
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }
    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.names.push(name(&node.name()));
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        if self.pattern_depth == 0 {
            self.names.push(name(&node.name()));
        }
    }
    fn visit_match_write_node(&mut self, node: &ruby_prism::MatchWriteNode<'pr>) {
        self.visit_call_node(&node.call());
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        self.pattern_depth += 1;
        self.visit(&node.pattern());
        self.pattern_depth -= 1;
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
        self.pattern_depth += 1;
        self.visit(&node.pattern());
        self.pattern_depth -= 1;
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
        self.pattern_depth += 1;
        self.visit(&node.pattern());
        self.pattern_depth -= 1;
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        // Targets inside a pattern guard's masgn would be lvasgn too.
        let depth = std::mem::replace(&mut self.pattern_depth, 0);
        ruby_prism::visit_multi_write_node(self, node);
        self.pattern_depth = depth;
    }
}

/// Pattern variables (`match_var`) below an `in` clause.
struct MatchVars {
    names: Vec<String>,
    pattern_depth: usize,
}

impl<'pr> Visit<'pr> for MatchVars {
    fn visit_local_variable_target_node(&mut self, node: &ruby_prism::LocalVariableTargetNode<'pr>) {
        if self.pattern_depth > 0 {
            self.names.push(name(&node.name()));
        }
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        self.pattern_depth += 1;
        self.visit(&node.pattern());
        self.pattern_depth -= 1;
        if let Some(s) = node.statements() {
            self.visit_statements_node(&s);
        }
    }
    fn visit_match_required_node(&mut self, node: &ruby_prism::MatchRequiredNode<'pr>) {
        self.visit(&node.value());
        self.pattern_depth += 1;
        self.visit(&node.pattern());
        self.pattern_depth -= 1;
    }
    fn visit_match_predicate_node(&mut self, node: &ruby_prism::MatchPredicateNode<'pr>) {
        self.visit(&node.value());
        self.pattern_depth += 1;
        self.visit(&node.pattern());
        self.pattern_depth -= 1;
    }
    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        let depth = std::mem::replace(&mut self.pattern_depth, 0);
        ruby_prism::visit_multi_write_node(self, node);
        self.pattern_depth = depth;
    }
}

impl<'pr> Visit<'pr> for Walker {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let k = match kind::of(&node) {
            kind::PROGRAM_NODE | kind::STATEMENTS_NODE | kind::ARGUMENTS_NODE | kind::ELSE_NODE => Fk::Transparent,
            kind::DEF_NODE => Fk::Def,
            kind::LAMBDA_NODE => {
                let l = node.as_lambda_node().unwrap();
                let params = l.parameters();
                let plain = params.as_ref().is_none_or(|p| p.as_block_parameters_node().is_some());
                Fk::Block { plain, bare: params.is_none() }
            }
            kind::CALL_NODE => match node.as_call_node().unwrap().block().and_then(|b| b.as_block_node()) {
                Some(b) => {
                    let params = b.parameters();
                    let plain = params.as_ref().is_none_or(|p| p.as_block_parameters_node().is_some());
                    Fk::Block { plain, bare: params.is_none() }
                }
                None => Fk::Other,
            },
            _ => Fk::Other,
        };
        self.push(key_of(&node), k);
    }

    fn visit_branch_node_leave(&mut self) {
        self.frames.pop();
    }

    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        let Some(block) = node.block().and_then(|b| b.as_block_node()) else {
            self.check_send(node);
            return ruby_prism::visit_call_node(self, node);
        };
        // (block (send recv args) (args) body): the send is a child frame of the block.
        let (s, e) = send_range(node);
        self.push((s, e, kind::CALL_NODE), Fk::Other);
        self.check_send(node);
        if let Some(r) = node.receiver() {
            self.visit(&r);
        }
        if let Some(a) = node.arguments() {
            self.visit_arguments_node(&a);
        }
        self.frames.pop();
        self.add_params(block.parameters());
        if let Some(p) = block.parameters() {
            self.visit(&p);
        }
        if let Some(b) = block.body() {
            self.visit(&b);
        }
    }

    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        self.add_params(node.parameters().map(|p| p.as_node()));
        ruby_prism::visit_def_node(self, node);
    }

    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.add_params(node.parameters());
        ruby_prism::visit_lambda_node(self, node);
    }

    fn visit_local_variable_write_node(&mut self, node: &ruby_prism::LocalVariableWriteNode<'pr>) {
        self.add_lhs(&node.value(), &name(&node.name()));
        ruby_prism::visit_local_variable_write_node(self, node);
    }

    fn visit_local_variable_or_write_node(&mut self, node: &ruby_prism::LocalVariableOrWriteNode<'pr>) {
        self.add_lhs(&node.value(), &name(&node.name()));
        ruby_prism::visit_local_variable_or_write_node(self, node);
    }

    fn visit_local_variable_and_write_node(&mut self, node: &ruby_prism::LocalVariableAndWriteNode<'pr>) {
        self.add_lhs(&node.value(), &name(&node.name()));
        ruby_prism::visit_local_variable_and_write_node(self, node);
    }

    fn visit_multi_write_node(&mut self, node: &ruby_prism::MultiWriteNode<'pr>) {
        let value = node.value();
        for t in node.lefts().iter().chain(node.rights().iter()) {
            if let Some(t) = t.as_local_variable_target_node() {
                self.add_lhs(&value, &name(&t.name()));
            }
        }
        ruby_prism::visit_multi_write_node(self, node);
    }

    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        if !self.guards.contains(&loc(&node.as_node())) {
            self.on_conditional(&node.as_node(), &node.predicate());
        }
        ruby_prism::visit_if_node(self, node);
    }

    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        if !self.guards.contains(&loc(&node.as_node())) {
            self.on_conditional(&node.as_node(), &node.predicate());
        }
        ruby_prism::visit_unless_node(self, node);
    }

    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'pr>) {
        if !node.is_begin_modifier() {
            self.on_conditional(&node.as_node(), &node.predicate());
        }
        ruby_prism::visit_while_node(self, node);
    }

    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'pr>) {
        if !node.is_begin_modifier() {
            self.on_conditional(&node.as_node(), &node.predicate());
        }
        ruby_prism::visit_until_node(self, node);
    }

    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        let pattern = node.pattern();
        if pattern.as_if_node().is_some() || pattern.as_unless_node().is_some() {
            self.guards.insert(loc(&pattern));
        }
        let mut m = MatchVars { names: Vec::new(), pattern_depth: 0 };
        m.visit_in_node(node);
        for n in m.names {
            self.write_self(n);
        }
        ruby_prism::visit_in_node(self, node);
    }

    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode<'pr>) {
        // Parser resbodies are siblings: this clause's frame ends before the next one.
        let (s, e) = loc(&node.as_node());
        self.push((s, e, kind::RESCUE_NODE), Fk::Other);
        if let Some(r) = node.reference().and_then(|r| r.as_local_variable_target_node()) {
            self.write_self(name(&r.name()));
        }
        for x in node.exceptions().iter() {
            self.visit(&x);
        }
        if let Some(r) = node.reference() {
            self.visit(&r);
        }
        if let Some(st) = node.statements() {
            self.visit_statements_node(&st);
        }
        self.frames.pop();
        if let Some(next) = node.subsequent() {
            self.visit_rescue_node(&next);
        }
    }
}

impl Cop for RedundantSelf {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut w = Walker {
            frames: Vec::new(),
            names: HashMap::new(),
            guards: HashSet::new(),
            offenses: Vec::new(),
        };
        w.visit(node);
        for (s, e, ds, de) in w.offenses {
            cx.add_fix(s, e, "Redundant `self` detected.", |c| {
                c.remove(s, e);
                c.remove(ds, de);
            });
        }
    }
}
