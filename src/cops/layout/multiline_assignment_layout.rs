use crate::helpers::check_assignment;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineAssignmentLayout";
pub static COP: MultilineAssignmentLayout = MultilineAssignmentLayout;
pub struct MultilineAssignmentLayout;

const NEW_LINE_OFFENSE: &str = "Right hand side of multi-line assignment is on the same line as the assignment operator `=`.";
const SAME_LINE_OFFENSE: &str =
    "Right hand side of multi-line assignment is not on the same line as the assignment operator `=`.";

/// The parser type of `rhs` (SupportedTypes may name any type), and its block `loc.begin`/`loc.end`.
fn rhs_type(n: &Node) -> Option<(&'static str, Option<(usize, usize)>)> {
    let ty = block_or_keyword_type(n);
    if ty.is_some() {
        return ty;
    }
    Some((helpers::literal_node::parser_type(n), None)).filter(|(t, _)| *t != "other")
}

fn block_or_keyword_type(n: &Node) -> Option<(&'static str, Option<(usize, usize)>)> {
    let block = |b: Option<Node>| -> Option<(&'static str, Option<(usize, usize)>)> {
        let b = b?.as_block_node()?;
        let t = match b.parameters() {
            Some(Node::NumberedParametersNode { .. }) => "numblock",
            Some(Node::ItParametersNode { .. }) => "itblock",
            _ => "block",
        };
        Some((t, Some((b.opening_loc().start_offset(), b.closing_loc().start_offset()))))
    };
    match n {
        Node::CallNode { .. } => block(n.as_call_node().unwrap().block()),
        Node::SuperNode { .. } => block(n.as_super_node().unwrap().block()),
        Node::ForwardingSuperNode { .. } => block(n.as_forwarding_super_node().unwrap().block().map(|b| b.as_node())),
        Node::LambdaNode { .. } => {
            let l = n.as_lambda_node().unwrap();
            let t = match l.parameters() {
                Some(Node::NumberedParametersNode { .. }) => "numblock",
                Some(Node::ItParametersNode { .. }) => "itblock",
                _ => "block",
            };
            Some((t, Some((l.opening_loc().start_offset(), l.closing_loc().start_offset()))))
        }
        Node::CaseNode { .. } => Some(("case", None)),
        Node::ClassNode { .. } => Some(("class", None)),
        Node::ModuleNode { .. } => Some(("module", None)),
        Node::IfNode { .. } | Node::UnlessNode { .. } => Some(("if", None)),
        Node::BeginNode { .. } if n.as_begin_node().unwrap().begin_keyword_loc().is_some() => Some(("kwbegin", None)),
        _ => None,
    }
}

impl Cop for MultilineAssignmentLayout {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        check_assignment::KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let operator = match node.as_call_node() {
            // `return if node.send_type? && node.loc.operator&.source != '='`
            Some(call) => {
                if is_csend(&call) {
                    return;
                }
                match call.equal_loc() {
                    Some(l) => l,
                    None => return,
                }
            }
            None => match assignment_parts(node) {
                Some((op, _)) => op,
                None => return,
            },
        };
        let Some(rhs) = check_assignment::extract_rhs(node, false) else { return };
        // `cop_config['SupportedTypes'].flat_map`: a String raises (a cop error).
        let supported = cx.cfg.array_bare("SupportedTypes");
        let Some((ty, block)) = rhs_type(&rhs) else { return };
        if !supported.iter().any(|t| t == ty || (t == "block" && matches!(ty, "numblock" | "itblock"))) {
            return;
        }
        let src = cx.src;
        let rl = rhs.location();
        let (rs, re) = (rl.start_offset(), rl.end_offset());
        let single = match block {
            Some((open, close)) => src.line_of(open) == src.line_of(close),
            None => src.line_of(rs) == src.line_of(re),
        };
        let start = node.location().start_offset();
        if single && (ty != "block" || block.is_some_and(|(open, _)| src.line_of(start) == src.line_of(open))) {
            return;
        }
        let (os, oe) = (operator.start_offset(), operator.end_offset());
        let (ns, ne) = (start, node.location().end_offset());
        let same = src.line_of(os) == src.line_of(rs);
        match cx.cfg.str("EnforcedStyle") {
            Some("new_line") if same => cx.add_fix(ns, ne, NEW_LINE_OFFENSE, |c| c.insert_after(os, oe, "\n")),
            Some("same_line") if !same => cx.add_fix(ns, ne, SAME_LINE_OFFENSE, |c| c.replace(oe, rs, " ")),
            _ => {}
        }
    }
}
