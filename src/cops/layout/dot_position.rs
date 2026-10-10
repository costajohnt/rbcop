use crate::prelude::*;
use ruby_prism::Location;

pub const NAME: &str = "Layout/DotPosition";
pub static COP: DotPosition = DotPosition;
pub struct DotPosition;

impl Cop for DotPosition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        // Parser's op_asgn / or_asgn / and_asgn / mlhs wrap a `send` that on_send also sees.
        &[
            kind::CALL_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::CALL_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (receiver, dot, selector || loc.begin)
        let (receiver, dot, selector): (Option<Node>, Option<Location>, Option<Location>) = match kind::of(node) {
            kind::CALL_NODE => {
                let c = node.as_call_node().unwrap();
                (c.receiver(), c.call_operator_loc(), c.message_loc().or(c.opening_loc()))
            }
            kind::CALL_OPERATOR_WRITE_NODE => {
                let c = node.as_call_operator_write_node().unwrap();
                (c.receiver(), c.call_operator_loc(), c.message_loc())
            }
            kind::CALL_OR_WRITE_NODE => {
                let c = node.as_call_or_write_node().unwrap();
                (c.receiver(), c.call_operator_loc(), c.message_loc())
            }
            kind::CALL_AND_WRITE_NODE => {
                let c = node.as_call_and_write_node().unwrap();
                (c.receiver(), c.call_operator_loc(), c.message_loc())
            }
            _ => {
                let c = node.as_call_target_node().unwrap();
                (Some(c.receiver()), Some(c.call_operator_loc()), Some(c.message_loc()))
            }
        };
        let (Some(receiver), Some(dot), Some(selector)) = (receiver, dot, selector) else { return };
        if !matches!(dot.as_slice(), b"." | b"&.") {
            return;
        }
        let src = cx.src;
        let selector_line = src.line_of(selector.start_offset());
        if selector_line == src.line_of(receiver.location().end_offset()) {
            return;
        }
        let dot_line = src.line_of(dot.start_offset());
        if selector_line as isize - receiver_end_line(src, &receiver).max(dot_line) as isize > 1 {
            return;
        }
        let leading = match cx.cfg.str("EnforcedStyle").unwrap_or("leading") {
            "leading" => true,
            "trailing" => false,
            _ => return,
        };
        if (dot_line == selector_line) == leading {
            return;
        }
        let msg = if leading {
            format!("Place the {} on the next line, together with the method name.", text(&dot))
        } else {
            format!("Place the {} on the previous line, together with the method call receiver.", text(&dot))
        };
        cx.add(dot.start_offset(), dot.end_offset(), msg);
    }
}

fn heredoc_end_line(src: &crate::source::Source, node: &Node) -> Option<usize> {
    if !is_heredoc(node) {
        return None;
    }
    let closing = match kind::of(node) {
        kind::STRING_NODE => node.as_string_node().unwrap().closing_loc(),
        kind::INTERPOLATED_STRING_NODE => node.as_interpolated_string_node().unwrap().closing_loc(),
        kind::X_STRING_NODE => Some(node.as_x_string_node().unwrap().closing_loc()),
        _ => Some(node.as_interpolated_x_string_node().unwrap().closing_loc()),
    };
    closing.map(|c| src.line_of(c.start_offset()))
}

/// receiver_end_line: the last heredoc terminator among a call receiver's arguments, the
/// receiver's own heredoc terminator, or the receiver's last line.
fn receiver_end_line(src: &crate::source::Source, receiver: &Node) -> usize {
    let heredoc_line = match receiver.as_call_node() {
        // A call with a literal block is a `block` node in parser, not call_type?.
        Some(c) if c.block().is_none_or(|b| b.as_block_node().is_none()) => {
            send_args(&c).iter().filter_map(|a| heredoc_end_line(src, a)).max()
        }
        Some(_) => None,
        None => heredoc_end_line(src, receiver),
    };
    heredoc_line.unwrap_or_else(|| src.line_of(receiver.location().end_offset()))
}
