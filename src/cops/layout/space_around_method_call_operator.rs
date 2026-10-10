use crate::prelude::*;
use ruby_prism::Location;

pub const NAME: &str = "Layout/SpaceAroundMethodCallOperator";
pub static COP: SpaceAroundMethodCallOperator = SpaceAroundMethodCallOperator;
pub struct SpaceAroundMethodCallOperator;

fn check_space(cx: &mut Cx, begin: usize, end: usize) {
    if end <= begin {
        return;
    }
    if cx.src.slice(begin, end).bytes().all(|b| b == b' ' || b == b'\t') {
        cx.add_fix(begin, end, "Avoid using spaces around a method call operator.", |c| c.remove(begin, end));
    }
}

/// Parser `send`/`csend` with a `.` or `&.`: receiver, operator, selector (or `(` of `foo.()`).
fn check_call(cx: &mut Cx, receiver: Option<Node>, op: Option<Location>, selector: Option<Location>, open: Option<Location>) {
    let (Some(receiver), Some(op)) = (receiver, op) else { return };
    if !matches!(op.as_slice(), b"." | b"&.") {
        return;
    }
    check_space(cx, receiver.location().end_offset(), op.start_offset());
    if let Some(sel) = selector.or(open) {
        check_space(cx, op.end_offset(), sel.start_offset());
    }
}

impl Cop for SpaceAroundMethodCallOperator {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::CALL_TARGET_NODE,
            kind::CONSTANT_PATH_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match kind::of(node) {
            kind::CALL_NODE => {
                let c = node.as_call_node().unwrap();
                check_call(cx, c.receiver(), c.call_operator_loc(), c.message_loc(), c.opening_loc());
            }
            kind::CALL_OPERATOR_WRITE_NODE => {
                let c = node.as_call_operator_write_node().unwrap();
                check_call(cx, c.receiver(), c.call_operator_loc(), c.message_loc(), None);
            }
            kind::CALL_OR_WRITE_NODE => {
                let c = node.as_call_or_write_node().unwrap();
                check_call(cx, c.receiver(), c.call_operator_loc(), c.message_loc(), None);
            }
            kind::CALL_AND_WRITE_NODE => {
                let c = node.as_call_and_write_node().unwrap();
                check_call(cx, c.receiver(), c.call_operator_loc(), c.message_loc(), None);
            }
            kind::CALL_TARGET_NODE => {
                let c = node.as_call_target_node().unwrap();
                check_call(cx, Some(c.receiver()), Some(c.call_operator_loc()), Some(c.message_loc()), None);
            }
            _ => {
                // `Foo::Bar = 1` is a parser `casgn`, not a `const`.
                let start = node.location().start_offset();
                if cx.parent().is_some_and(|p| {
                    matches!(
                        kind::of(p),
                        kind::CONSTANT_PATH_WRITE_NODE
                            | kind::CONSTANT_PATH_OPERATOR_WRITE_NODE
                            | kind::CONSTANT_PATH_OR_WRITE_NODE
                            | kind::CONSTANT_PATH_AND_WRITE_NODE
                    ) && p.location().start_offset() == start
                }) {
                    return;
                }
                let c = node.as_constant_path_node().unwrap();
                check_space(cx, c.delimiter_loc().end_offset(), c.name_loc().start_offset());
            }
        }
    }
}
