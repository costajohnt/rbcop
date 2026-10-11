//! RuboCop's FirstElementLineBreak and MultilineElementLineBreaks mixins (with
//! EmptyLineCorrector.insert_before). Children are parser-gem node ranges `(start, end)`.

use crate::cop::Cx;
use ruby_prism::Node;

fn lines(cx: &Cx, (s, e): (usize, usize)) -> (usize, usize) {
    (cx.src.line_of(s), cx.src.line_of(e))
}

fn report(cx: &mut Cx, (s, e): (usize, usize), msg: &str) {
    cx.add_fix(s, e, msg, |c| c.insert_before(s, e, "\n"));
}

/// Parser node range of a Prism node.
pub fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// `check_method_line_break(node, children)`: `node_start` is the send/def start.
pub fn check_method_line_break(cx: &mut Cx, node_start: usize, children: &[(usize, usize)], ignore_last: bool, msg: &str) {
    let Some(&first) = children.first() else { return };
    // `node.source_range.source_line[0...limit.loc.column] =~ /\s*\(\s*$/`
    let line = cx.src.lines.get(cx.src.line_of(node_start) - 1).copied().unwrap_or("");
    let col = cx.src.column(first.0);
    let prefix: String = if cx.src.binary {
        String::from_utf8_lossy(&line.as_bytes()[..col.min(line.len())]).into_owned()
    } else {
        line.chars().take(col).collect()
    };
    // Ruby's `\s` (`\v` included, which is_ascii_whitespace leaves out).
    if !prefix.trim_end_matches(|c: char| c.is_ascii() && crate::helpers::parser_tokens::is_ruby_space(c as u8)).ends_with('(') {
        return;
    }
    check_children_line_break(cx, node_start, children, ignore_last, msg);
}

/// `check_children_line_break(node, children)`: `node_start` is the node's start offset.
pub fn check_children_line_break(cx: &mut Cx, node_start: usize, children: &[(usize, usize)], ignore_last: bool, msg: &str) {
    if children.is_empty() {
        return;
    }
    let line = cx.src.line_of(node_start);
    // `min_by(&:first_line)`: the first child with the smallest first line.
    let min = *children.iter().min_by_key(|c| cx.src.line_of(c.0)).unwrap();
    if line != cx.src.line_of(min.0) {
        return;
    }
    let max_line = children.iter().map(|&c| if ignore_last { cx.src.line_of(c.0) } else { lines(cx, c).1 }).max();
    if Some(line) == max_line {
        return;
    }
    report(cx, min, msg);
}

/// `node.arguments` of a send/csend/super with an unbraced trailing hash replaced by its pairs.
pub fn flattened_args(args: Vec<Node>) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = args.iter().map(range).collect();
    if let Some(h) = args.last().and_then(|a| a.as_keyword_hash_node()) {
        out.pop();
        out.extend(h.elements().iter().map(|e| range(&e)));
    }
    out
}

/// Parser `super` arguments (a `&blk` included).
pub fn super_args<'pr>(s: &ruby_prism::SuperNode<'pr>) -> Vec<Node<'pr>> {
    let mut args: Vec<Node> = s.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
    args.extend(s.block().filter(|b| b.as_block_argument_node().is_some()));
    args
}

/// The `AllowMultilineFinalElement` option.
pub fn ignore_last(cx: &Cx) -> bool {
    cx.cfg.bool("AllowMultilineFinalElement").unwrap_or(false)
}

/// Element ranges of a list of nodes.
pub fn ranges<'pr>(nodes: impl IntoIterator<Item = Node<'pr>>) -> Vec<(usize, usize)> {
    nodes.into_iter().map(|n| range(&n)).collect()
}

/// MultilineElementLineBreaks#check_line_breaks.
pub fn check_line_breaks(cx: &mut Cx, children: &[(usize, usize)], ignore_last: bool, msg: &str) {
    let (Some(&first), Some(&last)) = (children.first(), children.last()) else { return };
    let same = if ignore_last {
        cx.src.line_of(first.0) == cx.src.line_of(last.0)
    } else {
        cx.src.line_of(first.0) == lines(cx, last).1
    };
    if same {
        return;
    }
    let mut last_seen = 0;
    for &child in children {
        let (f, l) = lines(cx, child);
        if last_seen >= f {
            report(cx, child, msg);
        } else {
            last_seen = l;
        }
    }
}
