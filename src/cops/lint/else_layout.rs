use crate::prelude::*;

pub const NAME: &str = "Lint/ElseLayout";
pub static COP: ElseLayout = ElseLayout;
pub struct ElseLayout;

const MSG: &str = "Odd `else` layout detected. Did you mean to use `elsif`?";

/// The parts of a parser `if` node this cop reads.
struct If<'pr> {
    node: Node<'pr>,
    keyword: &'pr [u8],
    then: bool,
    /// The `else`/`elsif` keyword and the else branch: an ElseNode's statements or an elsif IfNode.
    else_kw: Option<(usize, usize)>,
    else_branch: Option<Node<'pr>>,
}

fn parts<'pr>(node: Node<'pr>) -> Option<If<'pr>> {
    let (keyword, then, subsequent) = match &node {
        Node::IfNode { .. } => {
            let n = node.as_if_node().unwrap();
            (n.if_keyword_loc()?.as_slice(), n.then_keyword_loc().is_some(), n.subsequent())
        }
        Node::UnlessNode { .. } => {
            let n = node.as_unless_node().unwrap();
            (n.keyword_loc().as_slice(), n.then_keyword_loc().is_some(), n.else_clause().map(|e| e.as_node()))
        }
        _ => return None,
    };
    let (else_kw, else_branch) = match subsequent {
        None => (None, None),
        Some(s) => match s.as_else_node() {
            Some(e) => {
                let kw = e.else_keyword_loc();
                (Some((kw.start_offset(), kw.end_offset())), e.statements().map(|s| s.as_node()))
            }
            None => {
                let kw = s.as_if_node().unwrap().if_keyword_loc().unwrap();
                (Some((kw.start_offset(), kw.end_offset())), Some(s))
            }
        },
    };
    Some(If { node, keyword, then, else_kw, else_branch })
}

impl Cop for ElseLayout {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        // Re-borrow the node with the tree's lifetime.
        let node = match node {
            Node::IfNode { .. } => node.as_if_node().unwrap().as_node(),
            _ => node.as_unless_node().unwrap().as_node(),
        };
        let Some(p) = parts(node) else { return }; // ternary
        let multi_stmt = |b: &Option<Node>| b.as_ref().and_then(|b| b.as_statements_node()).is_some_and(|s| s.body().iter().count() > 1);
        if p.then && !multi_stmt(&p.else_branch) {
            return;
        }
        let start = p.node.location().start_offset();
        if cx.src.line_of(start) == cx.src.line_of(helpers::if_node::parser_end(&p.node)) {
            return;
        }
        check(cx, p);
    }
}

fn check(cx: &mut Cx, p: If) {
    let Some(else_branch) = p.else_branch else { return };
    let (else_start, else_end) = p.else_kw.unwrap();
    if &cx.src.text[else_start..else_end] == "else" {
        let first_else = match else_branch.as_statements_node() {
            Some(s) => s.body().iter().next().unwrap(),
            None => else_branch,
        };
        let loc = first_else.location();
        if cx.src.line_of(loc.start_offset()) != cx.src.line_of(else_start) {
            return;
        }
        let width = helpers::alignment::configured_indentation_width(cx) as usize;
        let indentation = " ".repeat(cx.src.column(p.node.location().start_offset()) + width);
        let first_start = loc.start_offset();
        cx.add_fix(loc.start_offset(), loc.end_offset(), MSG, |c| {
            c.insert_after(else_start, else_end, "\n");
            c.replace(else_end, first_start, &indentation);
        });
    } else if p.keyword == b"if"
        && let Some(next) = parts(else_branch) {
            check(cx, next);
        }
}
