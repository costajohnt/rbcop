use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/EmptyCaseCondition";
pub static COP: EmptyCaseCondition = EmptyCaseCondition;
pub struct EmptyCaseCondition;

struct HasReturn(bool);

impl<'pr> Visit<'pr> for HasReturn {
    fn visit_return_node(&mut self, _: &ruby_prism::ReturnNode<'pr>) {
        self.0 = true;
    }
}

/// NOT_SUPPORTED_PARENT_TYPES (return break next send csend yield super): the case is a call's
/// receiver, or an argument of a call, return, break, next, yield or super.
fn unsupported_parent(ancestors: &[Node]) -> bool {
    matches!(ancestors.last().map(kind::of), Some(kind::CALL_NODE | kind::ARGUMENTS_NODE))
}

fn parenthesize(node: &Node) -> String {
    let src = node_text(node);
    let wrap = is_assignment(node)
        || matches!(
            kind::of(node),
            kind::IF_NODE | kind::UNLESS_NODE | kind::AND_NODE | kind::OR_NODE | kind::RANGE_NODE | kind::FLIP_FLOP_NODE
        );
    if wrap { format!("({src})") } else { src.to_string() }
}

impl Cop for EmptyCaseCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_node().unwrap();
        if case.predicate().is_some() || unsupported_parent(cx.ancestors) {
            return;
        }
        let whens: Vec<ruby_prism::WhenNode> = case.conditions().iter().filter_map(|w| w.as_when_node()).collect();
        let mut ret = HasReturn(false);
        for w in &whens {
            if let Some(s) = w.statements() {
                ret.visit_statements_node(&s);
            }
        }
        if let Some(s) = case.else_clause().and_then(|e| e.statements()) {
            ret.visit_statements_node(&s);
        }
        if ret.0 {
            return;
        }
        let Some(first_when) = whens.first() else { return };
        let src = cx.src;
        let kw = case.case_keyword_loc();
        let (ks, ke) = (kw.start_offset(), kw.end_offset());
        // case_range = case keyword joined with the first when keyword
        let case_end = first_when.keyword_loc().end_offset();
        let column = src.column(ks);
        let indent = " ".repeat(column);
        let (first_line, last_line) = (src.line_of(ks), src.line_of(case_end));
        let mut comments = String::new();
        for line in first_line..last_line {
            if let Some(c) = src.comments.iter().rev().find(|c| src.line_of(c.start) == line) {
                comments.push_str(&format!("{indent}{}\n", src.slice(c.start, c.end)));
            }
        }
        let line_beginning = src.line_start(first_line);
        let has_grandparent = !parser_ancestors(cx.ancestors).is_empty();
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for w in whens.iter().skip(1) {
            let k = w.keyword_loc();
            edits.push((k.start_offset(), k.end_offset(), "elsif".to_string()));
        }
        for w in &whens {
            let conds: Vec<Node> = w.conditions().iter().collect();
            let (Some(first), Some(last)) = (conds.first(), conds.last()) else { continue };
            if has_grandparent && let Some(then) = w.then_keyword_loc() {
                edits.push((last.location().end_offset(), then.end_offset(), "\n".to_string()));
            }
            if conds.len() > 1 {
                let joined = conds.iter().map(parenthesize).collect::<Vec<_>>().join(" || ");
                edits.push((first.location().start_offset(), last.location().end_offset(), joined));
            }
        }
        cx.add_fix(ks, ke, "Do not use empty `case` condition, instead use an `if` expression.", |c| {
            c.replace(ks, case_end, "if");
            c.insert_before(line_beginning, case_end, &comments);
            for (s, e, t) in &edits {
                c.replace(*s, *e, t);
            }
        });
    }
}
