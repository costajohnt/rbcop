use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/SkipWithoutReason";
pub static COP: SkipWithoutReason = SkipWithoutReason;
pub struct SkipWithoutReason;

impl Cop for SkipWithoutReason {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(args) = receiverless(&call, &["skip"]) else { return };
        if is_csend(&call) {
            return;
        }
        let blank = match args.first() {
            None => true,
            Some(m) => m.as_string_node().is_some_and(|s| s.unescaped().is_empty()) && !is_parser_dstr(m),
        };
        if !blank {
            return;
        }
        // A call with a literal block is a parser block: the skip send's parent is that block.
        let own_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        let parent = if own_block { None } else { helpers::parser_parent::parser_parent(cx.ancestors) };
        let conditional = match parent {
            Some(p @ (Node::IfNode { .. } | Node::UnlessNode { .. } | Node::CaseNode { .. })) => Some(p),
            Some(Node::WhenNode { .. }) => {
                let i = cx.ancestors.iter().rposition(|a| a.as_when_node().is_some()).unwrap();
                cx.ancestors[..i].iter().rev().find(|a| a.as_case_node().is_some())
            }
            _ => None,
        };
        if let Some(c) = conditional
            && !only_skip_branches(c)
        {
            return;
        }
        let rescue_parent = match parent {
            Some(Node::RescueNode { .. }) => true,
            Some(Node::RescueModifierNode { .. }) => {
                let r = parent.unwrap().as_rescue_modifier_node().unwrap().rescue_expression();
                range(&r) == range(node)
            }
            _ => false,
        };
        if rescue_parent && !own_block {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Add a reason explaining why the test is skipped.");
    }
}

/// `only_skip_branches?`: two or more non-nil branches, each a `skip` send.
fn only_skip_branches(node: &Node) -> bool {
    let mut branches: Vec<Option<Node>> = Vec::new();
    fn body<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Node<'pr>> {
        s.map(|s| s.as_node())
    }
    match node {
        Node::IfNode { .. } => {
            let mut n = node.as_if_node().unwrap();
            loop {
                branches.push(body(n.statements()));
                match n.subsequent() {
                    Some(e) if e.as_if_node().is_some() => n = e.as_if_node().unwrap(),
                    Some(e) => {
                        branches.push(e.as_else_node().and_then(|e| body(e.statements())));
                        break;
                    }
                    None => break,
                }
            }
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            branches.push(body(n.statements()));
            if let Some(e) = n.else_clause() {
                branches.push(body(e.statements()));
            }
        }
        Node::CaseNode { .. } => {
            let n = node.as_case_node().unwrap();
            for w in n.conditions().iter() {
                branches.push(w.as_when_node().and_then(|w| body(w.statements())));
            }
            if let Some(e) = n.else_clause() {
                branches.push(body(e.statements()));
            }
        }
        _ => {}
    }
    let branches: Vec<Node> = branches.into_iter().flatten().filter_map(|b| parser_body(Some(b))).collect();
    branches.len() > 1 && branches.iter().all(|b| as_send(b).is_some_and(|c| method_name(&c) == "skip"))
}
