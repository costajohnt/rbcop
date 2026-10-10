use crate::helpers::nil_methods::nil_method;
use crate::helpers::node_types::is_operator_method;
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Lint/SafeNavigationConsistency";
pub static COP: SafeNavigationConsistency = SafeNavigationConsistency;
pub struct SafeNavigationConsistency;

struct Operand<'pr> {
    call: CallNode<'pr>,
    /// The operand's parent is an `and` (else an `or`).
    in_and: bool,
}

/// `collect_operands`: call operands of an and/or tree, left to right (parenthesized
/// operands are not entered).
fn collect<'pr>(node: &Node<'pr>, out: &mut Vec<Operand<'pr>>) {
    let (lhs, rhs, in_and) = match (node.as_and_node(), node.as_or_node()) {
        (Some(a), _) => (a.left(), a.right(), true),
        (_, Some(o)) => (o.left(), o.right(), false),
        _ => return,
    };
    for operand in [lhs, rhs] {
        if operand.as_and_node().is_some() || operand.as_or_node().is_some() {
            collect(&operand, out);
        } else if let Some(call) = operand.as_call_node()
            && call.block().is_none_or(|b| b.as_block_node().is_none())
        {
            out.push(Operand { call, in_and });
        }
    }
}

impl Cop for SafeNavigationConsistency {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::AND_NODE, kind::OR_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let mut operands = Vec::new();
        collect(node, &mut operands);
        // group_by receiver source, groups in order of first appearance
        let mut groups: Vec<(&str, Vec<&Operand>)> = Vec::new();
        for op in &operands {
            let key = op.call.receiver().map_or("", |r| node_text(&r));
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, g)) => g.push(op),
                None => groups.push((key, vec![op])),
            }
        }
        for (_, group) in groups {
            let (mut csend_and, mut csend_or, mut send_and, mut send_or) = (None, None, None, None);
            for (i, op) in group.iter().enumerate() {
                let csend = is_csend(&op.call);
                let nilable = csend || nil_method(cx.cfg, method_name(&op.call));
                let slot = |s: &mut Option<usize>, cond: bool| {
                    if cond && s.is_none() {
                        *s = Some(i);
                    }
                };
                slot(&mut csend_and, op.in_and && csend);
                slot(&mut csend_or, !op.in_and && csend);
                slot(&mut send_and, op.in_and && !nilable);
                slot(&mut send_or, !op.in_and && !nilable);
            }
            if let (Some(a), Some(o)) = (csend_and, csend_or)
                && a < o
            {
                continue;
            }
            let (dot_op, begin) = if let Some(ca) = csend_and {
                (".", send_and.map_or(ca, |s| s.min(ca)) + 1)
            } else if let (Some(so), Some(co)) = (send_or, csend_or) {
                if so < co { (".", so + 1) } else { ("&.", co + 1) }
            } else if let (Some(sa), Some(co)) = (send_and, csend_or)
                && sa < co
            {
                (".", co)
            } else {
                continue;
            };
            for op in group.iter().skip(begin) {
                let call = &op.call;
                let dot = call.call_operator_loc();
                let dot_text = dot.as_ref().map(|d| d.as_slice());
                let operator = is_operator_method(method_name(call));
                if dot_op == "&." && dot_text == Some(b"&.") {
                    continue;
                }
                if dot_op == "." && (dot_text == Some(b".") || operator) {
                    continue;
                }
                let msg = if dot_op == "." {
                    "Use `.` instead of unnecessary `&.`."
                } else {
                    "Use `&.` for consistency with safe navigation."
                };
                let (start, end) = match (&dot, operator) {
                    (_, true) => {
                        let l = call.location();
                        (l.start_offset(), l.end_offset())
                    }
                    (Some(d), false) => (d.start_offset(), d.end_offset()),
                    (None, false) => continue, // add_offense(nil) raises
                };
                let dot_range = dot.as_ref().map(|d| (d.start_offset(), d.end_offset()));
                cx.add_fix(start, end, msg, |c| {
                    if operator {
                        return;
                    }
                    if let Some((s, e)) = dot_range {
                        c.replace(s, e, dot_op);
                    }
                });
            }
        }
    }
}
