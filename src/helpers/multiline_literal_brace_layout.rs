//! RuboCop's MultilineLiteralBraceLayout mixin (plus what MultilineLiteralBraceCorrector edits).

use crate::cop::Cx;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::node_kind as kind;
use ruby_prism::{Node, Visit};

/// The cop's messages: SAME_LINE, NEW_LINE, ALWAYS_NEW_LINE, ALWAYS_SAME_LINE.
pub type Messages = [&'static str; 4];

/// rubocop-ast `node.chained? || node.argument?` for a node whose Prism parent is the last ancestor
/// (`argument?` needs a `send` parent, `chained?` any call). A call owning a `do`/`{}` block has the
/// block as parser parent, so callers pass `false` for those.
pub fn chained_or_argument(node: &Node, ancestors: &[Node]) -> bool {
    let n = ancestors.len();
    let Some(parent) = ancestors.last() else { return false };
    let loc = node.location();
    let same = |r: &Node| {
        let l = r.location();
        (l.start_offset(), l.end_offset()) == (loc.start_offset(), loc.end_offset())
    };
    if let Some(c) = parent.as_call_node() {
        return c.receiver().is_some_and(|r| same(&r));
    }
    kind::of(parent) == kind::ARGUMENTS_NODE
        && n >= 2
        && ancestors[n - 2].as_call_node().is_some_and(|c| !crate::ast::is_csend(&c))
}

/// `check_brace_layout` for a literal with an explicit opening brace at `open` and closing brace
/// `close` (byte ranges); `node` spans `node_range`; `children` in source order.
/// `heredoc_chain`: the `.method...` range the corrector moves for a call whose first argument is
/// a `str` heredoc (`correct_heredoc_argument_method_chain`), Err where RuboCop's corrector raises.
/// `node`: the Prism node of the literal (array, hash, call, or the def of a parameter list).
#[allow(clippy::too_many_arguments)]
pub fn check(
    cx: &mut Cx,
    msgs: &Messages,
    open: usize,
    close: (usize, usize),
    node_range: (usize, usize),
    children: &[Node],
    chained_or_argument: bool,
    heredoc_chain: Result<Option<(usize, usize)>, ()>,
    node: &Node,
) {
    let src = cx.src;
    let (Some(first), Some(last)) = (children.first(), children.last()) else { return };
    if src.line_of(node_range.0) == src.line_of(node_range.1) {
        return;
    }
    let last_end = last.location().end_offset();
    let last_line = src.line_of(last_end);
    // last_line_heredoc?(children.last)
    if crate::helpers::heredoc::last_heredoc_end(last).is_some_and(|(s, _)| src.line_of(s) >= last_line) {
        return;
    }
    let close_line = src.line_of(close.0);
    // heredoc_opened_on_closing_brace_line?: a descendant heredoc opens on the closing brace's
    // line (a heredoc whose opening lies inside the parser node's range is a descendant).
    let heredoc_on_close_line = || {
        src.root.as_ref().is_some_and(|root| {
            crate::helpers::heredoc::heredoc_starts(root)
                .into_iter()
                .any(|h| node_range.0 <= h && h < node_range.1 && src.line_of(h) == close_line)
        })
    };
    let closing_same = close_line == last_line;
    let opening_same = src.line_of(open) == src.line_of(first.location().start_offset());
    let msg = match cx.cfg.str("EnforcedStyle").unwrap_or("symmetrical") {
        "new_line" if closing_same => msgs[2],
        "same_line" if !closing_same => msgs[3],
        "symmetrical" if opening_same && !closing_same => msgs[0],
        "symmetrical" if !opening_same && closing_same => msgs[1],
        _ => return,
    };
    if heredoc_on_close_line() {
        return;
    }
    if closing_same {
        cx.add_fix(close.0, close.1, msg, |c| c.insert_before(close.0, close.1, "\n"));
        return;
    }
    // MultilineLiteralBraceCorrector, closing brace on a later line.
    let (_, after) = range_with_surrounding_space(src.text, last.location().start_offset(), last_end, right());
    let end_range = if src.text.as_bytes().get(after) == Some(&b',') { after + 1 } else { last_end };
    // new_line_needed_before_closing_brace?: the corrector does nothing.
    if src.comment_on_line(src.line_of(end_range)).is_some() && chained_or_argument {
        cx.add_fix(close.0, close.1, msg, |_| {});
        return;
    }
    if nested_closing_bracket_moved_before_comment(cx, node, last) {
        cx.add_fix(close.0, close.1, msg, |_| {});
        return;
    }
    let chain = match heredoc_chain {
        Err(()) => return, // RuboCop's corrector raises (`loc.dot` of a call without one)
        Ok(chain) => chain,
    };
    let (brace_start, _) = range_with_surrounding_space(src.text, close.0, close.1, left());
    let comment = src.comment_on_line(last_line).is_some();
    let node_line_end = {
        let l = src.line_of(node_range.1);
        let s = src.line_start(l);
        s + src.text[s..].find('\n').unwrap_or(src.text.len() - s)
    };
    let content = if comment { src.slice(close.0, node_line_end) } else { src.slice(close.0, close.1) };
    cx.add_fix(close.0, close.1, msg, |c| {
        c.remove(brace_start, close.1);
        if comment {
            c.remove(close.0, node_line_end);
        }
        c.insert_before(end_range, end_range, content);
        if let Some((s, e)) = chain {
            c.remove(s, e);
            c.insert_after(end_range, end_range, src.slice(s, e));
        }
    });
}

/// MultilineLiteralBraceCorrector#nested_closing_bracket_moved_before_comment?: a comment on the
/// line before the last element's last line, and a nested literal of the same kind whose closing
/// bracket starts that last line. The correction is skipped when the comment is on its own line,
/// or when the nested literal is neither chained nor an argument.
fn nested_closing_bracket_moved_before_comment(cx: &Cx, node: &Node, last: &Node) -> bool {
    let src = cx.src;
    let last_line = src.line_of(last.location().end_offset());
    let Some(comment) = last_line.checked_sub(1).and_then(|l| src.comment_on_line(l)) else { return false };
    // same_kind_as_node?: `descendant.type == node.type`, or both calls.
    // ponytail: for a def's `args`, a nested `args` (a lambda's `->(x)` in a default value) is
    // not matched; port it if a corpus ever shows one.
    let want = match kind::of(node) {
        k @ (kind::ARRAY_NODE | kind::HASH_NODE | kind::CALL_NODE) => k,
        _ => return false,
    };
    let container_is_send = node.as_call_node().is_some_and(|c| !crate::ast::is_csend(&c));
    struct Find<'a, 'pr> {
        src: &'a crate::source::Source<'a>,
        want: usize,
        last_line: usize,
        container_is_send: bool,
        stack: Vec<Node<'pr>>,
        /// `nested.chained? || nested.argument?` of the first match.
        found: Option<bool>,
    }
    impl<'pr> Visit<'pr> for Find<'_, 'pr> {
        fn visit_branch_node_enter(&mut self, d: Node<'pr>) {
            if self.found.is_none() && kind::of(&d) == self.want {
                let close = match &d {
                    Node::ArrayNode { .. } => d.as_array_node().unwrap().closing_loc(),
                    Node::HashNode { .. } => Some(d.as_hash_node().unwrap().closing_loc()),
                    _ => d.as_call_node().unwrap().closing_loc(),
                };
                let on_own_line = close.is_some_and(|c| {
                    matches!(c.as_slice(), b"}" | b"]" | b")")
                        && self.src.line_of(c.start_offset()) == self.last_line
                        && self.src.begins_its_line(c.start_offset())
                });
                if on_own_line {
                    // A call owning a block has the block as its parser parent.
                    let owns_block = d.as_call_node().is_some_and(|c| c.block().is_some_and(|b| b.as_block_node().is_some()));
                    self.found = Some(if owns_block {
                        false
                    } else if self.stack.is_empty() {
                        self.container_is_send
                    } else {
                        chained_or_argument(&d, &self.stack)
                    });
                }
            }
            self.stack.push(d);
        }
        fn visit_branch_node_leave(&mut self) {
            self.stack.pop();
        }
    }
    let mut f = Find { src, want, last_line, container_is_send, stack: Vec::new(), found: None };
    f.visit(last);
    let Some(chained_or_argument) = f.found else { return false };
    src.begins_its_line(comment.start) || !chained_or_argument
}

fn left() -> Surrounding {
    Surrounding { side: Side::Left, ..Default::default() }
}

fn right() -> Surrounding {
    Surrounding { side: Side::Right, ..Default::default() }
}
