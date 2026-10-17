use crate::prelude::*;
use helpers::parser_parent::parser_parent_index;
use helpers::variable_force::{self as vf, T, Tree};
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Rails/SaveBang";
pub static COP: SaveBang = SaveBang;
pub struct SaveBang;

const CREATE: &[&str] = &["create", "create_or_find_by", "first_or_create", "find_or_create_by"];
const MODIFY: &[&str] = &["save", "update", "update_attributes", "destroy"];

const MSG: &str = "Use `%<prefer>s` instead of `%<current>s` if the return value is not checked.";

fn message(template: &str, name: &str) -> String {
    template.replace("%<prefer>s", &format!("{name}!")).replace("%<current>s", name)
}

fn create_msg(name: &str) -> String {
    message(&format!("{MSG} Or check `persisted?` on model returned from `%<current>s`."), name)
}

/// rubocop-ast `literal?`
fn literal(n: &Node) -> bool {
    matches!(
        n,
        Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::XStringNode { .. }
            | Node::InterpolatedXStringNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::InterpolatedRegularExpressionNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::KeywordHashNode { .. }
            | Node::RangeNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::RationalNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
    )
}

/// receiver_chain_matches?: each `.`-separated part of an AllowedReceivers entry, right to left,
/// names the next receiver (a variable, a method, or a constant's trailing segments).
fn receiver_chain_matches(call: &CallNode, allowed: &str) -> bool {
    let mut node: Option<Node> = call.receiver();
    for part in allowed.split('.').collect::<Vec<_>>().into_iter().rev() {
        let Some(n) = node.take() else { return false };
        let ok = match &n {
            Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. } => node_text(&n) == part,
            Node::CallNode { .. } => {
                let c = n.as_call_node().unwrap();
                if is_csend(&c) || c.block().is_some_and(|b| b.as_block_node().is_some()) {
                    false
                } else {
                    method_name(&c) == part
                }
            }
            Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => {
                let name = helpers::const_name::const_name(&n).unwrap_or_default();
                let allowed: Vec<&str> = part.split("::").collect();
                let consts: Vec<&str> = name.split("::").collect();
                allowed.iter().rev().enumerate().all(|(i, a)| consts.iter().rev().nth(i).copied().unwrap_or("") == *a)
            }
            _ => false,
        };
        if !ok {
            return false;
        }
        // Variables and constants have no receiver: a further part fails.
        node = n.as_call_node().and_then(|c| c.receiver());
    }
    true
}

/// persist_method?: one of the methods, expected_signature?, and not an allowed receiver.
fn persist_method(cx: &Cx, call: &CallNode, methods: &[&str]) -> bool {
    let name = method_name(call);
    if !methods.contains(&name) {
        return false;
    }
    let args = send_args(call);
    if !args.is_empty() {
        if args.len() != 1 || name == "destroy" {
            return false;
        }
        let a = &args[0];
        if helpers::rails::hash_elements(a).is_none() && literal(a) {
            return false;
        }
    }
    // allowed_receiver?
    if let Some(r) = call.receiver() {
        if helpers::const_name::const_name(&r).as_deref() == Some("ENV") {
            return false;
        }
        if cx.cfg.get("AllowedReceivers").is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)))
            // `.any?`: a String raises (a cop error).
            && cx.cfg.array("AllowedReceivers").iter().any(|a| receiver_chain_matches(call, a))
        {
            return false;
        }
    }
    true
}

/// A parser `begin`: parentheses, or a statement list of two or more.
fn is_begin(n: &Node) -> bool {
    n.as_parentheses_node().is_some() || n.as_statements_node().is_some_and(|s| s.body().iter().count() > 1)
}

/// The parser children of a begin, as ranges.
fn begin_children(n: &Node) -> Vec<(usize, usize)> {
    let r = |x: &Node| (x.location().start_offset(), x.location().end_offset());
    if let Some(p) = n.as_parentheses_node() {
        return match p.body() {
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().map(|x| r(&x)).collect(),
                None => vec![r(&b)],
            },
            None => Vec::new(),
        };
    }
    n.as_statements_node().map(|s| s.body().iter().map(|x| r(&x)).collect()).unwrap_or_default()
}

fn assignment(n: &Node) -> bool {
    matches!(
        n,
        Node::LocalVariableWriteNode { .. }
            | Node::InstanceVariableWriteNode { .. }
            | Node::ClassVariableWriteNode { .. }
            | Node::GlobalVariableWriteNode { .. }
            | Node::ConstantWriteNode { .. }
            | Node::ConstantPathWriteNode { .. }
            | Node::MultiWriteNode { .. }
            | Node::LocalVariableOrWriteNode { .. }
            | Node::LocalVariableAndWriteNode { .. }
            | Node::LocalVariableOperatorWriteNode { .. }
            | Node::InstanceVariableOrWriteNode { .. }
            | Node::InstanceVariableAndWriteNode { .. }
            | Node::InstanceVariableOperatorWriteNode { .. }
            | Node::ClassVariableOrWriteNode { .. }
            | Node::ClassVariableAndWriteNode { .. }
            | Node::ClassVariableOperatorWriteNode { .. }
            | Node::GlobalVariableOrWriteNode { .. }
            | Node::GlobalVariableAndWriteNode { .. }
            | Node::GlobalVariableOperatorWriteNode { .. }
            | Node::ConstantOrWriteNode { .. }
            | Node::ConstantAndWriteNode { .. }
            | Node::ConstantOperatorWriteNode { .. }
            | Node::ConstantPathOrWriteNode { .. }
            | Node::ConstantPathAndWriteNode { .. }
            | Node::ConstantPathOperatorWriteNode { .. }
            | Node::CallOrWriteNode { .. }
            | Node::CallAndWriteNode { .. }
            | Node::CallOperatorWriteNode { .. }
            | Node::IndexOrWriteNode { .. }
            | Node::IndexAndWriteNode { .. }
            | Node::IndexOperatorWriteNode { .. }
    )
}

/// The parser node at `i` (== anc.len(): the visited call, with its block if it has one) as a
/// range.
fn range_at(anc: &[Node], node: &Node, i: usize) -> (usize, usize) {
    let n = if i == anc.len() { node } else { &anc[i] };
    // A BlockNode stands for parser's block, which starts at its call.
    if n.as_block_node().is_some() && i > 0 {
        return (anc[i - 1].location().start_offset(), n.location().end_offset());
    }
    (n.location().start_offset(), n.location().end_offset())
}

/// call_to_persisted?: a `persisted?` send (through a parenthesized call that is an `if`'s child,
/// to that `if`'s condition).
fn call_to_persisted(anc: &[Node], i: usize) -> bool {
    let n = &anc[i];
    let paren = n.as_call_node().is_some_and(|c| c.opening_loc().is_some_and(|o| o.as_slice() == b"("))
        || n.as_parentheses_node().is_some();
    let target: Option<&Node> = if paren {
        match parser_parent_index(anc, i).map(|p| &anc[p]) {
            Some(p) if p.as_if_node().is_some() || p.as_unless_node().is_some() => None,
            _ => Some(n),
        }
    } else {
        Some(n)
    };
    let Some(t) = target else {
        // The if's condition: persisted? only if the condition itself is that send.
        let p = &anc[parser_parent_index(anc, i).unwrap()];
        let cond = p.as_if_node().map(|x| x.predicate()).or_else(|| p.as_unless_node().map(|x| x.predicate())).unwrap();
        return helpers::rails::send_node(&cond).is_some_and(|c| method_name(&c) == "persisted?");
    };
    t.as_call_node().is_some_and(|c| !is_csend(&c) && method_name(&c) == "persisted?")
}

impl Cop for SaveBang {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(program) = node.as_program_node() {
            return check_assignments(cx, &program);
        }
        let call = node.as_call_node().unwrap();
        if !persist_method(cx, &call, &[CREATE, MODIFY].concat()) {
            return;
        }
        let anc = cx.ancestors;
        let len = anc.len();
        let name = method_name(&call);
        // assignable_node: the call (with its block), widened over enclosing hashes and arrays.
        let mut a = len;
        while let Some(p) = parser_parent_index(anc, a) {
            if anc[p].as_assoc_node().is_some()
                && let Some(h) = parser_parent_index(anc, p)
                && (anc[h].as_hash_node().is_some() || anc[h].as_keyword_hash_node().is_some())
            {
                a = h;
                continue;
            }
            if anc[p].as_array_node().is_some() {
                a = p;
                continue;
            }
            break;
        }
        let a_parent = parser_parent_index(anc, a);
        // return_value_assigned?
        if a_parent.is_some_and(|p| assignment(&anc[p])) {
            return;
        }
        // implicit_return?
        if cx.cfg.get("AllowImplicitReturn").is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false))) {
            // last_expression_in_begin
            let mut n = a;
            while let Some(p) = parser_parent_index(anc, n).filter(|&p| is_begin(&anc[p])) {
                let kids = begin_children(&anc[p]);
                if kids.last() != Some(&range_at(anc, node, n)) {
                    break;
                }
                n = p;
            }
            // find_method_with_sibling_index
            let mut sib = 1;
            let mut method = parser_parent_index(anc, n);
            while let Some(m) = method.filter(|&m| anc[m].as_or_node().is_some()) {
                sib += 1;
                method = parser_parent_index(anc, m);
            }
            if let Some(m) = method {
                let def = anc[m].as_def_node().is_some_and(|d| d.receiver().is_none());
                let block = matches!(anc[m], Node::BlockNode { .. } | Node::LambdaNode { .. });
                if def || block {
                    // node.sibling_index: within its parent, `n`'s index (an or's lhs 0 / rhs 1;
                    // a def's or block's body 2).
                    let idx = match parser_parent_index(anc, n).map(|p| &anc[p]) {
                        Some(Node::OrNode { .. }) => {
                            let o = anc[parser_parent_index(anc, n).unwrap()].as_or_node().unwrap();
                            if o.left().location().start_offset() == range_at(anc, node, n).0 { 0 } else { 1 }
                        }
                        _ => 2,
                    };
                    if 3 == idx + sib {
                        return;
                    }
                }
            }
        }
        // check_used_in_condition_or_compound_boolean?
        let mut p = parser_parent_index(anc, len);
        while let Some(i) = p.filter(|&i| is_begin(&anc[i])) {
            p = parser_parent_index(anc, i);
        }
        if let Some(i) = p {
            let par = &anc[i];
            let single_negative = par.as_call_node().is_some_and(|c| {
                !is_csend(&c)
                    && method_name(&c) == "!"
                    && send_args(&c).is_empty()
                    && c.receiver().is_some_and(|r| !helpers::rails::send_node(&r).is_some_and(|rc| method_name(&rc) == "!"))
            });
            let cond = match par {
                Node::IfNode { .. } => Some(par.as_if_node().unwrap().predicate()),
                Node::UnlessNode { .. } => Some(par.as_unless_node().unwrap().predicate()),
                Node::CaseNode { .. } => par.as_case_node().unwrap().predicate(),
                _ => None,
            };
            let deparen = cond.map(|mut c| {
                while let Some(pn) = c.as_parentheses_node() {
                    match pn.body().map(|b| helpers::rails::body_statements(Some(b))) {
                        Some(mut v) if !v.is_empty() => c = v.pop().unwrap(),
                        _ => break,
                    }
                }
                (c.location().start_offset(), c.location().end_offset())
            });
            let is_cond = deparen.is_some_and(|r| r == range_at(anc, node, len)) && !is_pattern_guard(par, &anc[..i]);
            if par.as_and_node().is_some() || par.as_or_node().is_some() || single_negative || is_cond {
                if !MODIFY.contains(&name) {
                    let sel = call.message_loc().unwrap();
                    let (s, e) = (sel.start_offset(), sel.end_offset());
                    let bang = format!("{name}!");
                    cx.add_fix(s, e, message("`%<current>s` returns a model which is always truthy.", name), |c| {
                        c.replace(s, e, &bang)
                    });
                }
                return;
            }
        }
        // argument?: an argument of a parent send.
        if let Some(p) = a_parent
            && let Some(pc) = anc[p].as_call_node()
            && !is_csend(&pc)
        {
            let r = range_at(anc, node, a);
            let in_args = send_args(&pc).iter().any(|x| (x.location().start_offset(), x.location().end_offset()) == r);
            let own_block = p + 1 < len && anc[p + 1].as_block_node().is_some();
            if in_args && !own_block {
                return;
            }
        }
        // explicit_return?
        if a_parent.is_some_and(|p| anc[p].as_return_node().is_some() || anc[p].as_next_node().is_some()) {
            return;
        }
        // checked_immediately?: node.parent (the call's own block when it has one).
        let own_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        if !own_block && parser_parent_index(anc, len).is_some_and(|i| call_to_persisted(anc, i)) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let bang = format!("{name}!");
        cx.add_fix(s, e, message(MSG, name), |c| c.replace(s, e, &bang));
    }
}

/// VariableForce after_leaving_scope / check_assignment: `x = Model.create(...)` whose variable is
/// never checked with `persisted?`.
fn check_assignments(cx: &mut Cx, program: &ruby_prism::ProgramNode) {
    let tree = Tree::shared(program, cx.src);
    // Calls by parser send range, to look the assigned send up in Prism.
    struct F<'pr>(Vec<CallNode<'pr>>);
    impl<'pr> Visit<'pr> for F<'pr> {
        fn visit_call_node(&mut self, c: &CallNode<'pr>) {
            self.0.push(c.as_node().as_call_node().unwrap());
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut calls = F(Vec::new());
    calls.visit(&program.as_node());
    let mut hits: Vec<(usize, usize, String)> = Vec::new();
    vf::investigate(tree, |tree, scope, vars| {
        for &v in &scope.variables {
            let var = &vars[v];
            for asgn in &var.assignments {
                if tree.t(asgn.node) != T::Lvasgn {
                    continue;
                }
                let Some(mut value) = tree.child(asgn.node, 0) else { continue };
                if tree.t(value).is_any_block() {
                    let Some(s) = tree.child(value, 0) else { continue };
                    value = s;
                }
                if tree.t(value) != T::Send || tree.nodes[value].csend {
                    continue;
                }
                let r = tree.range(value);
                let Some(call) = calls.0.iter().find(|c| send_range(c) == r) else { continue };
                if !persist_method(cx, call, CREATE) {
                    continue;
                }
                // persisted_referenced?
                // call_to_persisted?(reference.node.parent): a parenthesized call that is an `if`'s
                // child stands for that `if`'s condition.
                let persisted_send = |n: usize| tree.t(n) == T::Send && !tree.nodes[n].csend && tree.nodes[n].name == "persisted?";
                let persisted = asgn.referenced
                    && var.references.iter().any(|reference| {
                        let Some(p) = tree.parent(reference.node) else { return false };
                        let paren = tree.t(p) == T::Send
                            && calls.0.iter().find(|c| send_range(c) == tree.range(p)).is_some_and(|c| {
                                c.opening_loc().is_some_and(|o| o.as_slice() == b"(")
                            });
                        if paren && let Some(i) = tree.parent(p).filter(|&i| tree.t(i) == T::If) {
                            return tree.child(i, 0).is_some_and(persisted_send);
                        }
                        persisted_send(p)
                    });
                if persisted {
                    continue;
                }
                let sel = call.message_loc().unwrap();
                hits.push((sel.start_offset(), sel.end_offset(), method_name(call).to_string()));
            }
        }
    });
    for (s, e, name) in hits {
        let bang = format!("{name}!");
        cx.add_fix(s, e, create_msg(&name), |c| c.replace(s, e, &bang));
    }
}
