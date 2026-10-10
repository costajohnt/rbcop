//! RuboCop's TrailingComma mixin (Style/TrailingCommaIn{Arguments,ArrayLiteral,HashLiteral}).

use crate::prelude::*;

/// The parser node being checked, reduced to what the mixin asks of it.
pub struct Subject {
    /// `node.source_range`
    pub range: (usize, usize),
    /// `node_end_location`: `loc.end`, or the node's last character
    pub end_loc: (usize, usize),
    /// `elements(node)` ranges
    pub elements: Vec<(usize, usize)>,
    /// Call nodes: (`loc.selector` start or node start, last argument range, last argument is a
    /// braced hash).
    pub call: Option<(usize, (usize, usize), bool)>,
}

impl Subject {
    /// `check_literal` subject for an array or braced hash literal.
    pub fn literal(node: &Node, items: &[Node], end_loc: (usize, usize)) -> Subject {
        let l = node.location();
        Subject {
            range: (l.start_offset(), l.end_offset()),
            end_loc,
            elements: items.iter().map(range).collect(),
            call: None,
        }
    }
}

pub fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

static AFTER_LAST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A,?[ \t\r\n\x0c\x0b]*(#.*)?\n").unwrap());

/// `heredoc?`
fn heredoc(node: &Node) -> bool {
    if is_heredoc(node) {
        return true;
    }
    match node {
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            if is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
                return false;
            }
            // heredoc_send?: no arguments -> the receiver, otherwise the last argument
            match send_args(&call).last() {
                Some(last) => heredoc(last),
                None => call.receiver().is_some_and(|r| heredoc(&r)),
            }
        }
        Node::AssocNode { .. } => heredoc(&node.as_assoc_node().unwrap().value()),
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().last().is_some_and(|e| heredoc(&e)),
        Node::KeywordHashNode { .. } => {
            node.as_keyword_hash_node().unwrap().elements().iter().last().is_some_and(|e| heredoc(&e))
        }
        _ => false,
    }
}

/// `check(node, items, kind, begin_pos, end_pos)`; `kind` holds `{article}`.
pub fn check(cx: &mut Cx, subject: &Subject, items: &[Node], kind: &str, begin: usize, end: usize) {
    let src = cx.src;
    let after = src.slice(begin, end);
    let any_heredoc = items.iter().any(heredoc);
    let lead = after.bytes().take_while(|&b| matches!(b, b' ' | b'\t' | b'\r' | b'\x0c' | b'\x0b') || (b == b'\n' && !any_heredoc));
    let comma_offset = (after.as_bytes().get(lead.count()) == Some(&b',')).then(|| after.find(',').unwrap());
    let style = cx.cfg.str("EnforcedStyleForMultiline").unwrap_or("no_comma");
    let should_have = should_have_comma(cx, style, subject, items);
    let inside_comment = |offset: usize| {
        src.comment_on_line(src.line_of(begin)).is_some_and(|c| c.start < begin + offset)
    };
    match comma_offset {
        Some(offset) if !inside_comment(offset) => {
            if !should_have {
                avoid_comma(cx, style, kind, begin + offset);
            }
        }
        _ if should_have => put_comma(cx, items, kind),
        _ => {}
    }
}

fn should_have_comma(cx: &Cx, style: &str, s: &Subject, items: &[Node]) -> bool {
    let src = cx.src;
    let line = |o: usize| src.line_of(o);
    // parser `Range#last_line` is the line of `end_pos`
    let last_line = |r: (usize, usize)| line(r.1);
    // multiline?: node.multiline? && !allowed_multiline_argument?
    let multiline = line(s.range.0) != last_line(s.range)
        && !(s.elements.len() == 1 && !src.begins_its_line(s.end_loc.0));
    if !multiline {
        return false;
    }
    match style {
        "comma" => {
            // no_elements_on_same_line?
            let mut ranges = s.elements.clone();
            ranges.push(s.end_loc);
            ranges.windows(2).all(|w| last_line(w[0]) != line(w[1].0))
        }
        "consistent_comma" => {
            // !method_name_and_arguments_on_same_line?
            let Some((sel, last_arg, braced_hash)) = s.call else { return true };
            if last_line(s.range) != last_line(last_arg) {
                return true;
            }
            !(braced_hash || line(sel) == last_line(last_arg))
        }
        "diff_comma" => {
            // last_item_precedes_newline?
            let Some(last) = items.last() else { return false };
            AFTER_LAST.is_match(src.slice(range(last).1, s.range.1))
        }
        _ => false,
    }
}

fn avoid_comma(cx: &mut Cx, style: &str, kind: &str, pos: usize) {
    let article = if kind.contains("array") { "an" } else { "a" };
    let extra = match style {
        "comma" => ", unless each item is on its own line",
        "consistent_comma" => ", unless items are split onto multiple lines",
        "diff_comma" => ", unless that item immediately precedes a newline",
        _ => "",
    };
    let msg = format!("Avoid comma after the last {}{extra}.", kind.replace("{article}", article));
    cx.add_fix(pos, pos + 1, msg, |c| c.remove(pos, pos + 1));
}

fn put_comma(cx: &mut Cx, items: &[Node], kind: &str) {
    let Some(last) = items.last() else { return };
    if last.as_block_argument_node().is_some() {
        return;
    }
    // autocorrect_range: from the first non-space character of the item's last line
    let (s, e) = range(last);
    let text = cx.src.slice(s, e);
    let ix = text.rfind('\n').unwrap_or(0);
    let Some(off) = text[ix..].bytes().position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\x0b')) else {
        return;
    };
    let start = s + ix + off;
    let msg = format!("Put a comma after the last {}.", kind.replace("{article}", "a multiline"));
    let src = cx.src;
    cx.add_fix(start, e, msg, |c| {
        if src.slice(start, e) == "," {
            c.remove(start, e)
        } else {
            c.insert_after(start, e, ",")
        }
    });
}
