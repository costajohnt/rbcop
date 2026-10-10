use crate::helpers::parser_parent::parser_parent;
use crate::prelude::*;

pub const NAME: &str = "Style/AmbiguousEndlessMethodDefinition";
pub static COP: AmbiguousEndlessMethodDefinition = AmbiguousEndlessMethodDefinition;
pub struct AmbiguousEndlessMethodDefinition;

/// The single statement of an optional statement list.
fn only(stmts: Option<ruby_prism::StatementsNode>) -> Option<Node> {
    let mut it = stmts?.body().iter();
    let n = it.next()?;
    it.next().is_none().then_some(n)
}

impl Cop for AmbiguousEndlessMethodDefinition {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (3.0, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() || def.equal_loc().is_none() {
            return;
        }
        let Some(op) = parser_parent(cx.ancestors) else { return };
        let start = node.location().start_offset();
        let is_def = |n: Option<Node>| n.is_some_and(|n| n.location().start_offset() == start && n.as_def_node().is_some());
        // `^${(if _ <def _>) ({and or} def _) ({while until} _ def)}` in modifier form.
        let keyword = match op {
            Node::IfNode { .. } => {
                let i = op.as_if_node().unwrap();
                let modifier = i.if_keyword_loc().is_some() && i.end_keyword_loc().is_none();
                (modifier && i.subsequent().is_none() && is_def(only(i.statements()))).then(|| i.if_keyword_loc())
            }
            Node::UnlessNode { .. } => {
                let u = op.as_unless_node().unwrap();
                let modifier = u.end_keyword_loc().is_none();
                (modifier && u.else_clause().is_none() && is_def(only(u.statements()))).then(|| Some(u.keyword_loc()))
            }
            Node::WhileNode { .. } => {
                let w = op.as_while_node().unwrap();
                (w.closing_loc().is_none() && !w.is_begin_modifier() && is_def(only(w.statements())))
                    .then(|| Some(w.keyword_loc()))
            }
            Node::UntilNode { .. } => {
                let w = op.as_until_node().unwrap();
                (w.closing_loc().is_none() && !w.is_begin_modifier() && is_def(only(w.statements())))
                    .then(|| Some(w.keyword_loc()))
            }
            Node::AndNode { .. } => {
                let a = op.as_and_node().unwrap();
                is_def(Some(a.left())).then(|| Some(a.operator_loc()))
            }
            Node::OrNode { .. } => {
                let a = op.as_or_node().unwrap();
                is_def(Some(a.left())).then(|| Some(a.operator_loc()))
            }
            _ => None,
        };
        let Some(Some(keyword)) = keyword else { return };
        let msg = format!("Avoid using `{}` statements with endless methods.", text(&keyword));
        let Some(body) = def.body() else { return };
        let args = match (def.parameters(), def.lparen_loc(), def.rparen_loc()) {
            (Some(p), Some(l), Some(r)) if !parameter_nodes(&p).is_empty() => {
                cx.src.slice(l.start_offset(), r.end_offset()).to_string()
            }
            _ => String::new(),
        };
        let indent = " ".repeat(cx.src.column(start));
        let width = helpers::alignment::configured_indentation_width(cx).max(0) as usize;
        let replacement = format!(
            "def {}{args}\n{indent}{}{}\n{indent}end",
            text(&def.name_loc()),
            " ".repeat(width),
            node_text(&body)
        );
        let (ds, de) = (start, node.location().end_offset());
        let loc = op.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| c.replace(ds, de, &replacement));
    }
}
