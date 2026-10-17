use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/ChangeByZero";
pub static COP: ChangeByZero = ChangeByZero;
pub struct ChangeByZero;

fn change_method(name: &str) -> bool {
    matches!(name, "change" | "a_block_changing" | "changing")
}

/// `(send _ :by (int 0))`: the `.by(0)` send.
fn by_zero<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    let by = node.as_call_node().filter(|c| !is_csend(c) && method_name(c) == "by")?;
    let [arg] = &send_args(&by)[..] else { return None };
    (arg.as_integer_node().is_some() && int_literal_value(arg) == Some(0)).then_some(by)
}

/// expect_change_with_arguments: `(send $(send nil? CHANGE_METHODS ...) :by (int 0))`
fn with_arguments<'pr>(by: &CallNode<'pr>) -> Option<CallNode<'pr>> {
    let recv = by.receiver()?;
    helpers::rspec::send_call(&recv).filter(|c| c.receiver().is_none() && change_method(method_name(c)))
}

/// expect_change_with_block: `(send (block $(send nil? CHANGE_METHODS) (args) (send (...) _)) :by (int 0))`
fn with_block<'pr>(by: &CallNode<'pr>) -> Option<CallNode<'pr>> {
    let recv = by.receiver()?;
    let change = helpers::rspec::plain_block_call(&recv)?;
    if change.receiver().is_some() || !change_method(method_name(&change)) || !send_args(&change).is_empty() {
        return None;
    }
    let block = helpers::rspec::block_of(&change)?;
    if block.parameters().is_some_and(|p| {
        p.as_block_parameters_node().is_none_or(|b| b.parameters().is_some() || b.locals().iter().next().is_some())
    }) {
        return None;
    }
    let stmts: Vec<Node> = block.body()?.as_statements_node()?.body().iter().collect();
    let [stmt] = &stmts[..] else { return None };
    let send = helpers::rspec::send_call(stmt)?;
    (!is_csend(&send) && send.receiver().is_some() && send_args(&send).is_empty()).then_some(change)
}

impl Cop for ChangeByZero {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_send for the change send; `.by(0)` is its parent (the block's parent in the block form).
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !change_method(method_name(&call)) {
            return;
        }
        let anc = cx.ancestors;
        let Some(pi) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return };
        let Some(by) = by_zero(&anc[pi]) else { return };
        let (s, e) = helpers::rspec::node_range(&by.as_node());
        if by.receiver().is_none_or(|r| helpers::rspec::node_range(&r) != helpers::rspec::node_range(node)) {
            return;
        }
        let change = if helpers::rspec::block_of(&call).is_some() { with_block(&by) } else { with_arguments(&by) };
        let Some(change) = change else { return };
        // register_offense: `return unless node.parent.send_type?` (a block around `.by(0)` is not)
        if helpers::rspec::block_of(&by).is_some() {
            return;
        }
        let Some(ppi) = helpers::parser_parent::parser_parent_index(anc, pi) else { return };
        let Some(parent) = anc[ppi].as_call_node().filter(|c| !is_csend(c)) else { return };
        let src = cx.src;
        let dot_s = by.call_operator_loc().unwrap().start_offset();
        let change_name = method_name(&change).to_string();
        let operator = method_name(&parent);
        if !matches!(operator, "and" | "or" | "&" | "|") {
            let psel = parent.message_loc().map(|l| (l.start_offset(), l.end_offset()));
            let csel = change.message_loc().unwrap();
            let (cs, ce) = (csel.start_offset(), csel.end_offset());
            cx.add_fix(s, e, format!("Prefer `not_to change` over `to {change_name}.by(0)`."), |c| {
                if let Some((ps, pe)) = psel {
                    c.replace(ps, pe, "not_to");
                }
                c.replace(cs, ce, "change");
                c.remove(dot_s, e);
            });
            return;
        }
        let negated = match cx.cfg.get("NegatedMatcher") {
            None | Some(Value::Null | Value::Bool(false)) => None,
            Some(v) => v.as_string(),
        };
        let preferred = negated.as_ref().map_or("negated matchers".to_string(), |n| format!("`{n}`"));
        let msg = format!("Prefer {preferred} with compound expectations over `{change_name}.by(0)`.");
        let Some(negated) = negated else {
            cx.add_fix(s, e, msg, |_| {});
            return;
        };
        // insert_operator: `node.right_siblings.first` is the operator only when `.by(0)` is the receiver
        let insert_op = parent.receiver().is_some_and(|r| helpers::rspec::node_range(&r) == (s, e))
            && matches!(operator, "&" | "|");
        let operator = format!(" {operator}");
        // change_nodes(node): every change send under `.by(0)`, in order; replace_node is the send
        // (argument form) or its block, i.e. the whole Prism call.
        struct V(Vec<((usize, usize), (usize, usize))>);
        impl<'pr> Visit<'pr> for V {
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                if !is_csend(n) && n.receiver().is_none() && change_method(method_name(n)) {
                    let sel = n.message_loc().unwrap();
                    self.0.push(((sel.start_offset(), sel.end_offset()), helpers::rspec::node_range(&n.as_node())));
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V(Vec::new());
        v.visit(&by.as_node());
        let edits: Vec<_> = v
            .0
            .into_iter()
            .map(|(sel, whole)| {
                let removal = if src.line_of(whole.0) == src.line_of(dot_s) {
                    (dot_s, e)
                } else {
                    helpers::whole_lines::range_by_whole_lines(src.text, dot_s, e, true)
                };
                (sel, whole, removal)
            })
            .collect();
        cx.add_fix(s, e, msg, |c| {
            for &((ss, se), (ns, ne), (rs, re)) in &edits {
                c.replace(ss, se, &negated);
                if insert_op {
                    c.insert_after(ns, ne, &operator);
                }
                c.remove(rs, re);
            }
        });
    }
}
