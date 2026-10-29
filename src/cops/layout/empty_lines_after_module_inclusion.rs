use crate::cops::layout_empty_lines_around_attribute_accessor::{empty_line_insertion, next_line_empty_or_enable_directive};
use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAfterModuleInclusion";
pub static COP: EmptyLinesAfterModuleInclusion = EmptyLinesAfterModuleInclusion;
pub struct EmptyLinesAfterModuleInclusion;

const MSG: &str = "Add an empty line after module inclusion.";

fn inclusion_name(name: &str) -> bool {
    matches!(name, "include" | "extend" | "prepend")
}

/// `allowed_method?`: the sibling (or a modifier form's body) is a plain send of an inclusion method.
fn body<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Option<Node<'pr>> {
    s.and_then(|s| s.body().iter().next())
}

fn allowed(node: &Node) -> bool {
    let node = if let Some(i) = node.as_if_node()
        && i.if_keyword_loc().is_some()
        && i.end_keyword_loc().is_none()
    {
        body(i.statements())
    } else if let Some(u) = node.as_unless_node()
        && u.end_keyword_loc().is_none()
    {
        body(u.statements())
    } else if let Some(w) = node.as_while_node()
        && w.closing_loc().is_none()
    {
        body(w.statements())
    } else if let Some(w) = node.as_until_node()
        && w.closing_loc().is_none()
    {
        body(w.statements())
    } else {
        return inclusion_send(node);
    };
    node.is_some_and(|n| inclusion_send(&n))
}

fn inclusion_send(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && c.block().is_none_or(|b| kind::of(&b) != kind::BLOCK_NODE)
            && inclusion_name(method_name(&c))
    })
}

impl Cop for EmptyLinesAfterModuleInclusion {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || !inclusion_name(method_name(&call)) || send_args(&call).is_empty() {
            return;
        }
        // Parent `block` (own block, or sole statement of a block body), `send` or `array`.
        if call.block().is_some_and(|b| kind::of(&b) == kind::BLOCK_NODE) {
            return;
        }
        let parent = helpers::parser_parent::parser_parent(cx.ancestors);
        if let Some(p) = parent
            && match frame(p) {
                Frame::Block => true,
                Frame::Call => !p.as_call_node().is_some_and(|c| is_csend(&c)),
                _ => kind::of(p) == kind::ARRAY_NODE,
            }
        {
            return;
        }
        let (start, end) = send_range(&call);
        if next_line_empty_or_enable_directive(cx.src, end) {
            return;
        }
        // `next_line_node`: RuboCop crashes without a parent; nil under if / rescue / ensure / and /
        // or (1.92.2), which (like every other single-statement parent here) leaves no right sibling.
        let Some(parent) = parent else { return };
        let sibling = if let Some(s) = parent.as_statements_node() {
            let body: Vec<Node> = s.body().iter().collect();
            body.iter().position(|n| n.location().start_offset() == start).and_then(|i| body.into_iter().nth(i + 1))
        } else if let Some(c) = parent.as_call_node().filter(|c| is_csend(c)) {
            // A csend parent is not skipped (only `send`): the next argument is the right sibling (#450).
            let args = send_args(&c);
            args.iter().position(|n| n.location().start_offset() == start).and_then(|i| args.into_iter().nth(i + 1))
        } else {
            None
        };
        if sibling.is_none_or(|s| allowed(&s)) {
            return;
        }
        let at = empty_line_insertion(cx.src, start, end);
        cx.add_fix(start, end, MSG, |c| c.insert_after(at.0, at.1, "\n"));
    }
}
