use crate::helpers::if_node::{Body, Else};
use crate::prelude::*;

pub const NAME: &str = "Style/EmptyStringInsideInterpolation";
pub static COP: EmptyStringInsideInterpolation = EmptyStringInsideInterpolation;
pub struct EmptyStringInsideInterpolation;

const MSG_TRAILING_CONDITIONAL: &str = "Do not use trailing conditionals in string interpolation.";
const MSG_TERNARY: &str = "Do not return empty strings in string interpolation.";

/// `empty_branch_outcome?`: a `nil` or `''` branch.
fn empty_outcome(body: Option<&Body>) -> bool {
    body.and_then(|b| b.single()).is_some_and(|n| match &n {
        Node::NilNode { .. } => true,
        Node::StringNode { .. } => !is_parser_dstr(&n) && n.as_string_node().unwrap().unescaped().is_empty(),
        _ => false,
    })
}

fn source<'a>(src: &'a str, body: Option<&Body>) -> &'a str {
    body.map_or("", |b| {
        let (s, e) = match b.single() {
            Some(n) => (n.location().start_offset(), n.location().end_offset()),
            None => b.range(),
        };
        &src[s..e]
    })
}

impl Cop for EmptyStringInsideInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::EMBEDDED_STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(stmts) = node.as_embedded_statements_node().unwrap().statements() else { return };
        let ternary_style = match cx.cfg.str("EnforcedStyle") {
            Some("trailing_conditional") => false,
            Some("ternary") => true,
            _ => return,
        };
        let src = cx.src.text;
        for child in stmts.body().iter() {
            let Some(cond) = helpers::if_node::cond(&child) else { continue };
            let (_, if_branch, els) = helpers::if_node::parts(&child);
            let else_branch = match &els {
                Some(Else::Body(b)) => b.as_ref(),
                _ => None,
            };
            let cond_src = node_text(&cond.condition);
            if ternary_style {
                if !cond.modifier() {
                    continue;
                }
                let branch = source(src, if_branch.as_ref());
                let component = if cond.keyword == "unless" {
                    format!("'' : {branch}")
                } else {
                    format!("{branch} : ''")
                };
                let replacement = format!("#{{{cond_src} ? {component}}}");
                let (s, e) = (node.location().start_offset(), node.location().end_offset());
                cx.add_fix(s, e, MSG_TRAILING_CONDITIONAL, |c| c.replace(s, e, &replacement));
                continue;
            }
            if cond.modifier() {
                continue;
            }
            let (s, e) = (child.location().start_offset(), child.location().end_offset());
            // An `elsif` else branch is an if node, never an empty outcome.
            let else_empty = !matches!(els, Some(Else::Elsif(_))) && empty_outcome(else_branch);
            if empty_outcome(if_branch.as_ref()) {
                if matches!(els, None | Some(Else::Body(None))) {
                    // `nil.source` raises: RuboCop drops the rest of this interpolation.
                    return;
                }
                let r = format!("{} unless {cond_src}", elsif_or_source(src, &els));
                cx.add_fix(s, e, MSG_TERNARY, |c| c.replace(s, e, &r));
            }
            if else_empty {
                if if_branch.is_none() {
                    return;
                }
                let r = format!("{} if {cond_src}", source(src, if_branch.as_ref()));
                cx.add_fix(s, e, MSG_TERNARY, |c| c.replace(s, e, &r));
            }
        }
    }
}

/// `else_branch.source`: an `elsif` node (parser range) or the else body.
fn elsif_or_source<'a>(src: &'a str, els: &Option<Else>) -> &'a str {
    match els {
        Some(Else::Elsif(n)) => &src[n.location().start_offset()..helpers::if_node::parser_end(n)],
        Some(Else::Body(b)) => source(src, b.as_ref()),
        None => "",
    }
}
