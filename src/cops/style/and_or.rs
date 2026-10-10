use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/AndOr";
pub static COP: AndOr = AndOr;
pub struct AndOr;

/// `and`/`or` keyword operator of an AndNode/OrNode: (start, end, current, preferred).
fn keyword_operator(node: &Node) -> Option<(usize, usize, &'static str, &'static str)> {
    let op = node.as_and_node().map(|n| n.operator_loc()).or_else(|| node.as_or_node().map(|n| n.operator_loc()))?;
    let (cur, alt) = match op.as_slice() {
        b"and" => ("and", "&&"),
        b"or" => ("or", "||"),
        _ => return None,
    };
    Some((op.start_offset(), op.end_offset(), cur, alt))
}

fn report(cx: &mut Cx, node: &Node) {
    if let Some((s, e, cur, alt)) = keyword_operator(node) {
        cx.add(s, e, format!("Use `{alt}` instead of `{cur}`."));
    }
}

/// Collects every node of a subtree (`each_node`), keeping the and/or ones.
struct Collect<'pr>(Vec<Node<'pr>>);

impl<'pr> Visit<'pr> for Collect<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        if node.as_and_node().is_some() || node.as_or_node().is_some() {
            self.0.push(node);
        }
    }
}

impl Cop for AndOr {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::AND_NODE, kind::OR_NODE, kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let always = cx.cfg.str("EnforcedStyle") == Some("always");
        let predicate = if let Some(n) = node.as_if_node() {
            n.predicate()
        } else if let Some(n) = node.as_unless_node() {
            n.predicate()
        } else if let Some(n) = node.as_while_node() {
            n.predicate()
        } else if let Some(n) = node.as_until_node() {
            n.predicate()
        } else {
            if always {
                report(cx, node);
            }
            return;
        };
        if always || is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let mut found = Collect(Vec::new());
        found.visit(&predicate);
        for n in &found.0 {
            report(cx, n);
        }
    }
}
