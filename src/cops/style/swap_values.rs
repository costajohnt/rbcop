use crate::prelude::*;

pub const NAME: &str = "Style/SwapValues";
pub static COP: SwapValues = SwapValues;
pub struct SwapValues;

/// A simple assignment (lvasgn ivasgn cvasgn gvasgn casgn): (lhs, rhs source, start, end).
fn simple<'pr>(node: &Node<'pr>) -> Option<(String, &'pr str, usize, usize)> {
    let lhs = match node {
        Node::LocalVariableWriteNode { .. } => text(&node.as_local_variable_write_node()?.name_loc()).to_string(),
        Node::InstanceVariableWriteNode { .. } => text(&node.as_instance_variable_write_node()?.name_loc()).to_string(),
        Node::ClassVariableWriteNode { .. } => text(&node.as_class_variable_write_node()?.name_loc()).to_string(),
        Node::GlobalVariableWriteNode { .. } => text(&node.as_global_variable_write_node()?.name_loc()).to_string(),
        Node::ConstantWriteNode { .. } => text(&node.as_constant_write_node()?.name_loc()).to_string(),
        // `"#{'::' if node.absolute?}#{node.const_name}"`
        Node::ConstantPathWriteNode { .. } => node_text(&node.as_constant_path_write_node()?.target().as_node())
            .split_whitespace()
            .collect(),
        _ => return None,
    };
    let (_, value) = assignment_parts(node)?;
    let loc = node.location();
    Some((lhs, node_text(&value), loc.start_offset(), loc.end_offset()))
}

impl Cop for SwapValues {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::CLASS_VARIABLE_WRITE_NODE,
            kind::GLOBAL_VARIABLE_WRITE_NODE,
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // ponytail: right siblings only within statement lists; argument/array siblings are not
        // considered.
        let Some(stmts) = cx.parent().and_then(|p| p.as_statements_node()) else { return };
        let start = node.location().start_offset();
        // Statements start in increasing order: a binary search keeps a long list linear overall.
        let body = stmts.body();
        let (mut lo, mut hi) = (0, body.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if body.get(mid).unwrap().location().start_offset() < start { lo = mid + 1 } else { hi = mid }
        }
        if body.get(lo).is_none_or(|n| n.location().start_offset() != start) {
            return;
        }
        let (Some(x), Some(y)) = (body.get(lo + 1), body.get(lo + 2)) else { return };
        let (Some(tmp), Some(x), Some(y)) = (simple(node), simple(&x), simple(&y)) else { return };
        if !(x.0 == tmp.1 && y.0 == x.1 && y.1 == tmp.0) {
            return;
        }
        let replacement = format!("{0}, {1} = {1}, {0}", x.0, x.1);
        let msg = format!(
            "Replace this and assignments at lines {} and {} with `{replacement}`.",
            cx.src.line_of(x.2),
            cx.src.line_of(y.2)
        );
        let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, tmp.2, y.3, false);
        cx.add_fix(tmp.2, tmp.3, msg, |c| c.replace(rs, re, &replacement));
    }
}
