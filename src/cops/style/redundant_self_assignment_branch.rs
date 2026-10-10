use crate::helpers::if_node::{Body, Else, parts};
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantSelfAssignmentBranch";
pub static COP: RedundantSelfAssignmentBranch = RedundantSelfAssignmentBranch;
pub struct RedundantSelfAssignmentBranch;

impl Cop for RedundantSelfAssignmentBranch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::LOCAL_VARIABLE_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let asgn = node.as_local_variable_write_node().unwrap();
        let expression = asgn.value();
        // `if_type?`: if, unless, ternary (every form has an if and an else branch to look at).
        if !matches!(kind::of(&expression), kind::IF_NODE | kind::UNLESS_NODE) {
            return;
        }
        let (condition, if_branch, else_part) = parts(&expression);
        let else_branch = match else_part {
            Some(Else::Elsif(_)) => return,
            Some(Else::Body(b)) => b,
            None => None,
        };
        if multiple_statements(&if_branch) || multiple_statements(&else_branch) {
            return;
        }
        let name = asgn.name().as_slice();
        let if_node = if_branch.as_ref().and_then(Body::single);
        let else_node = else_branch.as_ref().and_then(Body::single);
        let self_assign = |b: &Option<Node>| b.as_ref().is_some_and(|n| node_text(n).as_bytes() == name);
        let (offense, opposite, keyword) = if self_assign(&if_node) {
            (if_node.unwrap(), else_node, "unless")
        } else if self_assign(&else_node) {
            (else_node.unwrap(), if_node, "if")
        } else {
            return;
        };
        let src = cx.src.text;
        let mut replacement = format!(
            "{} {keyword} {}",
            opposite.as_ref().map_or("nil", node_text),
            node_text(&condition)
        );
        if let Some((opening, end, _)) = opposite.as_ref().and_then(helpers::heredoc::parts) {
            replacement.push_str(&src[opening.1..end.1]);
        }
        let (s, e) = (expression.location().start_offset(), expression.location().end_offset());
        let loc = offense.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Remove the self-assignment branch.", |c| {
            c.replace(s, e, &replacement)
        });
    }
}

/// `branch.begin_type? && !branch.children.empty?`
fn multiple_statements(branch: &Option<Body>) -> bool {
    let Some(b) = branch else { return false };
    match b.single() {
        None => true,
        Some(n) => n.as_parentheses_node().is_some_and(|p| p.body().is_some()),
    }
}
