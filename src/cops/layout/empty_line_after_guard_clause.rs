use crate::prelude::*;
use crate::helpers::if_node::{guard_clause, guard_clause_branch, if_branch};
use crate::source::Source;

pub const NAME: &str = "Layout/EmptyLineAfterGuardClause";
pub static COP: EmptyLineAfterGuardClause = EmptyLineAfterGuardClause;
pub struct EmptyLineAfterGuardClause;

const MSG: &str = "Add empty line after guard clause.";

static SIMPLECOV: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A#\s*(?::nocov:|simplecov\s*:\s*(?:disable|enable)\b)").unwrap());

fn is_heredoc_str(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE | kind::X_STRING_NODE | kind::INTERPOLATED_X_STRING_NODE
    ) && is_heredoc(node)
}

/// parser `children.last` of a guard-ish node, when it is a node.
fn last_child<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    match kind::of(node) {
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            if c.block().is_some_and(|b| b.as_block_node().is_some()) {
                // (block send args body): the body.
                return c.block().unwrap().as_block_node().unwrap().body();
            }
            send_args(&c).pop()
        }
        kind::RETURN_NODE => node.as_return_node().unwrap().arguments().and_then(|a| a.arguments().iter().last()),
        kind::BREAK_NODE => node.as_break_node().unwrap().arguments().and_then(|a| a.arguments().iter().last()),
        kind::NEXT_NODE => node.as_next_node().unwrap().arguments().and_then(|a| a.arguments().iter().last()),
        kind::AND_NODE => Some(node.as_and_node().unwrap().right()),
        kind::OR_NODE => Some(node.as_or_node().unwrap().right()),
        _ => None,
    }
}

/// `respond_to?(:arguments)` nodes' arguments (send/csend, super, yield, return, break, next).
fn arguments_of<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    let args = |a: Option<ruby_prism::ArgumentsNode<'pr>>| a.map(|a| a.arguments().iter().collect()).unwrap_or_default();
    Some(match kind::of(node) {
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            if c.block().is_some_and(|b| b.as_block_node().is_some()) {
                return None;
            }
            send_args(&c)
        }
        kind::SUPER_NODE => args(node.as_super_node().unwrap().arguments()),
        kind::YIELD_NODE => args(node.as_yield_node().unwrap().arguments()),
        kind::RETURN_NODE => args(node.as_return_node().unwrap().arguments()),
        kind::BREAK_NODE => args(node.as_break_node().unwrap().arguments()),
        kind::NEXT_NODE => args(node.as_next_node().unwrap().arguments()),
        _ => return None,
    })
}

fn has_heredoc_descendant(node: &Node) -> bool {
    struct V {
        root: bool,
        found: bool,
    }
    impl V {
        fn check(&mut self, node: &Node) {
            if !std::mem::take(&mut self.root) {
                self.found |= is_heredoc_str(node);
            }
        }
    }
    impl<'pr> ruby_prism::Visit<'pr> for V {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            self.check(&node);
        }
        fn visit_leaf_node_enter(&mut self, node: Node<'pr>) {
            self.check(&node);
        }
    }
    let mut v = V { root: true, found: false };
    ruby_prism::Visit::visit(&mut v, node);
    v.found
}

/// last_heredoc_argument (recursive).
fn last_heredoc_argument<'pr>(node: Option<Node<'pr>>) -> Option<Node<'pr>> {
    let node = node?;
    // last_heredoc_argument_node
    let mut n = if matches!(kind::of(&node), kind::IF_NODE | kind::UNLESS_NODE) {
        let cond = match node.as_if_node() {
            Some(i) => i.predicate(),
            None => node.as_unless_node().unwrap().predicate(),
        };
        // ponytail: an if/else with a multi-statement branch (`begin`) is not followed further.
        let branch = if_branch(&node)?;
        if let Some(a) = branch.as_and_node() {
            Some(a.left())
        } else if has_heredoc_descendant(&cond) {
            Some(cond)
        } else {
            last_child(&branch)
        }
    } else {
        Some(node)
    }?;
    while let Some(p) = n.as_parentheses_node() {
        let body = p.body()?;
        n = match body.as_statements_node() {
            Some(s) => s.body().iter().next()?,
            None => body,
        };
    }
    if is_heredoc_str(&n) {
        return Some(n);
    }
    let args = arguments_of(&n)?;
    for a in args {
        if let Some(h) = last_heredoc_argument(Some(a)) {
            return Some(h);
        }
    }
    let receiver = n.as_call_node().and_then(|c| c.receiver());
    last_heredoc_argument(receiver)
}

/// (body first line, terminator line, terminator range without newline) of a heredoc.
fn heredoc_parts(src: &Source, node: &Node) -> (usize, usize, (usize, usize)) {
    let (closing, first) = match kind::of(node) {
        kind::STRING_NODE => {
            let s = node.as_string_node().unwrap();
            (s.closing_loc().unwrap(), Some(s.content_loc().start_offset()))
        }
        kind::INTERPOLATED_STRING_NODE => {
            let s = node.as_interpolated_string_node().unwrap();
            (s.closing_loc().unwrap(), s.parts().iter().next().map(|p| p.location().start_offset()))
        }
        kind::X_STRING_NODE => {
            let s = node.as_x_string_node().unwrap();
            (s.closing_loc(), Some(s.content_loc().start_offset()))
        }
        _ => {
            let s = node.as_interpolated_x_string_node().unwrap();
            (s.closing_loc(), s.parts().iter().next().map(|p| p.location().start_offset()))
        }
    };
    let term_line = src.line_of(closing.start_offset());
    let term_start = src.line_start(term_line);
    let term_end = src.text[term_start..].find('\n').map_or(src.text.len(), |i| term_start + i);
    let first_line = first.filter(|&f| f < term_start).map_or(term_line, |f| src.line_of(f));
    (first_line, term_line, (term_start, term_end))
}

fn blank_line(src: &Source, line: usize) -> Option<bool> {
    // processed_source[line] (0-based) is the 1-based line `line + 1`.
    src.lines.get(line).map(|l| helpers::ruby_string::blank(l))
}

/// Comment starting on 1-based `line` that is an enable directive or a simplecov directive.
fn allowed_directive_comment(src: &Source, line: usize) -> Option<(usize, usize)> {
    let c = src.comment_on_line(line)?;
    let text = src.slice(c.start, c.end).trim_end_matches('\n');
    let enabled = crate::directives::parse(text).is_some_and(|d| d.mode == "enable" || d.mode == "enable-next");
    (enabled || SIMPLECOV.is_match(text)).then_some((c.start, c.start + text.len()))
}

/// None: RuboCop crashes (`nil.blank?` past the last line).
fn next_line_empty_or_allowed_directive_comment(src: &Source, line: usize) -> Option<bool> {
    if blank_line(src, line)? {
        return Some(true);
    }
    if allowed_directive_comment(src, line + 1).is_some() {
        return blank_line(src, line + 1);
    }
    Some(false)
}

impl Cop for EmptyLineAfterGuardClause {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let src = cx.src;
        // correct_style?: the guard must be followed by a sibling in a `begin`.
        let Some(branch) = if_branch(node) else { return };
        if !guard_clause(src, &branch) {
            return;
        }
        let Some(parent) = cx.parent().and_then(|p| p.as_statements_node()) else { return };
        let siblings: Vec<Node> = parent.body().iter().collect();
        let start = node.location().start_offset();
        let Some(i) = siblings.iter().position(|s| s.location().start_offset() == start) else { return };
        let Some(next) = siblings.get(i + 1) else { return };
        if matches!(kind::of(next), kind::IF_NODE | kind::UNLESS_NODE)
            && if_branch(next).is_some_and(|b| guard_clause(src, &b) || guard_clause_branch(&b))
        {
            return;
        }
        // multiple_statements_on_line?
        if src.line_of(start) == src.line_of(next.location().start_offset()) {
            return;
        }
        let end = node.location().end_offset();
        let last_line = src.line_of(end);
        let (keyword, end_kw) = match kind::of(node) {
            kind::IF_NODE => {
                let n = node.as_if_node().unwrap();
                (n.if_keyword_loc(), n.end_keyword_loc())
            }
            _ => {
                let n = node.as_unless_node().unwrap();
                (Some(n.keyword_loc()), n.end_keyword_loc())
            }
        };
        let modifier = keyword.is_some() && end_kw.is_none();
        let me = match node.as_if_node() {
            Some(n) => n.as_node(),
            None => node.as_unless_node().unwrap().as_node(),
        };
        let heredoc = if modifier { last_heredoc_argument(Some(me)) } else { None };
        let text = src.text;
        if let Some(h) = heredoc {
            let (first, term, range) = heredoc_parts(src, &h);
            let line = last_line + (term - first) + 1;
            if next_line_empty_or_allowed_directive_comment(src, line) != Some(false) {
                return;
            }
            let fix = fix_range(src, text, src.line_start(first), range.1);
            cx.add_fix(range.0, range.1, MSG, |c| c.insert_after(fix.0, fix.1, "\n"));
        } else {
            if next_line_empty_or_allowed_directive_comment(src, last_line) != Some(false) {
                return;
            }
            let (s, e) = match end_kw {
                Some(k) => (k.start_offset(), k.end_offset()),
                None => (start, end),
            };
            let fix = fix_range(src, text, start, end);
            cx.add_fix(s, e, MSG, |c| c.insert_after(fix.0, fix.1, "\n"));
        }
    }
}

/// The autocorrect range: whole lines of the range, or the allowed directive comment after it.
fn fix_range(src: &Source, text: &str, start: usize, end: usize) -> (usize, usize) {
    let range = helpers::whole_lines::range_by_whole_lines(text, start, end, false);
    let next_line = src.line_of(range.1) + 1;
    allowed_directive_comment(src, next_line).unwrap_or(range)
}
