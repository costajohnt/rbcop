use crate::prelude::*;
use ruby_prism::{ItLocalVariableReadNode, LocalVariableReadNode, Visit};

pub const NAME: &str = "Style/ItBlockParameter";
pub static COP: ItBlockParameter = ItBlockParameter;
pub struct ItBlockParameter;

const MSG_USE: &str = "Use `it` block parameter.";
const MSG_AVOID: &str = "Avoid using `it` block parameter.";
const MSG_AVOID_MULTILINE: &str = "Avoid using `it` block parameter for multi-line blocks.";

/// `lvar` nodes (by source) in a subtree.
struct Lvars<'a> {
    name: &'a str,
    out: Vec<(usize, usize)>,
}

impl<'pr> Visit<'pr> for Lvars<'_> {
    fn visit_local_variable_read_node(&mut self, node: &LocalVariableReadNode<'pr>) {
        // `d.source == name`: parser's lvar is the bare name even for `foo(name:)`.
        if node.name().as_slice() == self.name.as_bytes() {
            self.out.push(helpers::lvar::range(&node.location(), self.name.as_bytes()));
        }
    }
    fn visit_it_local_variable_read_node(&mut self, node: &ItLocalVariableReadNode<'pr>) {
        if self.name == "it" {
            self.out.push(helpers::lvar::range(&node.location(), b"it"));
        }
    }
}

/// `node.body.each_descendant(:lvar).select { |d| d.source == name }`: a single-statement body is
/// the parser body itself, which `each_descendant` excludes.
fn find_block_variables(body: Option<Node>, name: &str) -> Vec<(usize, usize)> {
    let Some(body) = body else { return Vec::new() };
    let mut v = Lvars { name, out: Vec::new() };
    let single = body.as_statements_node().and_then(|s| {
        let stmts: Vec<Node> = s.body().iter().collect();
        if stmts.len() == 1 { stmts.into_iter().next() } else { None }
    });
    match single {
        Some(stmt) => {
            let l = stmt.location();
            v.visit(&stmt);
            v.out.retain(|&r| r != (l.start_offset(), l.end_offset()));
        }
        None => v.visit(&body),
    }
    v.out
}

impl Cop for ItBlockParameter {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.4, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        // Block owners rather than BlockNode: parser visits a block before its receiver chain.
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let block = match node {
            Node::CallNode { .. } => node.as_call_node().unwrap().block(),
            Node::SuperNode { .. } => node.as_super_node().unwrap().block(),
            Node::ForwardingSuperNode { .. } => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
            _ => None,
        };
        let (params, body, opening, closing) = if let Some(l) = node.as_lambda_node() {
            (l.parameters(), l.body(), l.opening_loc(), l.closing_loc())
        } else {
            let Some(b) = block.as_ref().and_then(|b| b.as_block_node()) else { return };
            (b.parameters(), b.body(), b.opening_loc(), b.closing_loc())
        };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("allow_single_line");
        match params.as_ref() {
            Some(Node::NumberedParametersNode { .. }) => {
                if style == "disallow" || params.unwrap().as_numbered_parameters_node().unwrap().maximum() != 1 {
                    return;
                }
                for (s, e) in find_block_variables(body, "_1") {
                    cx.add_fix(s, e, MSG_USE, |c| c.replace(s, e, "it"));
                }
            }
            Some(Node::ItParametersNode { .. }) => match style {
                "allow_single_line" => {
                    if cx.src.line_of(opening.start_offset()) == cx.src.line_of(closing.start_offset()) {
                        return;
                    }
                    let l = node.location();
                    cx.add_with(l.start_offset(), l.end_offset(), MSG_AVOID_MULTILINE, false);
                }
                "disallow" => {
                    for (s, e) in find_block_variables(body, "it") {
                        cx.add_with(s, e, MSG_AVOID, false);
                    }
                }
                _ => {}
            },
            Some(Node::BlockParametersNode { .. }) if style == "always" => {
                let bp = params.as_ref().unwrap().as_block_parameters_node().unwrap();
                let Some(p) = bp.parameters() else { return };
                let rest_empty = p.optionals().iter().next().is_none()
                    && parser_rest(p.rest()).is_none()
                    && p.posts().iter().next().is_none()
                    && p.keywords().iter().next().is_none()
                    && p.keyword_rest().is_none()
                    && p.block().is_none()
                    && bp.locals().iter().next().is_none();
                let reqs: Vec<Node> = p.requireds().iter().collect();
                let [arg] = reqs.as_slice() else { return };
                if !rest_empty || arg.as_required_parameter_node().is_none() {
                    return;
                }
                let name = node_text(arg);
                let pl = bp.location();
                let (ps, pe) = (pl.start_offset(), pl.end_offset());
                for (s, e) in find_block_variables(body, name) {
                    cx.add_fix(s, e, MSG_USE, |c| {
                        c.remove(ps, pe);
                        c.replace(s, e, "it");
                    });
                }
            }
            _ => {}
        }
    }
}
