//! Util#add_parentheses, computed up front so it can run inside an `add_fix` closure.

use crate::ast::{send_args, send_range};
use crate::cop::Corrector;
use crate::node_kind as kind;
use ruby_prism::Node;

pub enum ParenFix {
    /// Remove the character after the selector/keyword, put `(` there and `)` at the node's end.
    Args { selector_end: usize, char_len: usize, end: usize },
    /// `insert_after(node, '()')`
    Empty { start: usize, end: usize },
    /// `wrap(node, '(', ')')`
    Wrap { start: usize, end: usize },
}

/// None where RuboCop would raise (a call without a selector, such as `foo.()`).
pub fn plan(node: &Node, text: &str) -> Option<ParenFix> {
    let (keyword_end, has_args, (start, end)) = match kind::of(node) {
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            (c.message_loc().map(|l| l.end_offset()), !send_args(&c).is_empty(), send_range(&c))
        }
        kind::SUPER_NODE => {
            let s = node.as_super_node().unwrap();
            let has = s.arguments().is_some() || s.block().is_some_and(|b| b.as_block_argument_node().is_some());
            let l = node.location();
            (Some(s.keyword_loc().end_offset()), has, (l.start_offset(), l.end_offset()))
        }
        kind::YIELD_NODE => {
            let y = node.as_yield_node().unwrap();
            let l = node.location();
            (Some(y.keyword_loc().end_offset()), y.arguments().is_some(), (l.start_offset(), l.end_offset()))
        }
        _ => {
            let l = node.location();
            return Some(ParenFix::Wrap { start: l.start_offset(), end: l.end_offset() });
        }
    };
    if !has_args {
        return Some(ParenFix::Empty { start, end });
    }
    let selector_end = keyword_end?;
    let char_len = text[selector_end..].chars().next().map_or(0, char::len_utf8);
    Some(ParenFix::Args { selector_end, char_len, end })
}

pub fn apply(c: &mut Corrector, fix: &ParenFix) {
    match *fix {
        ParenFix::Args { selector_end, char_len, end } => {
            c.remove(selector_end, selector_end + char_len);
            c.insert_before(selector_end, selector_end + char_len, "(");
            c.insert_after(end, end, ")");
        }
        ParenFix::Empty { start, end } => c.insert_after(start, end, "()"),
        ParenFix::Wrap { start, end } => c.wrap(start, end, "(", ")"),
    }
}
