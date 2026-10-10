use crate::prelude::*;

pub const NAME: &str = "Style/RedundantStringEscape";
pub static COP: RedundantStringEscape = RedundantStringEscape;
pub struct RedundantStringEscape;

/// What `allowed_escape?` needs to know about the parser `str` (or its enclosing literal).
struct Ctx {
    /// `[loc.begin.source[-1], loc.end.source[0]]` of the literal that `delimiter?` ends up at;
    /// None for heredocs (never a delimiter).
    delims: Option<(char, char)>,
    /// An escaped space is fine: `percent_array_literal?(node) || node.heredoc?`.
    space_ok: bool,
}

fn first_char(s: &str) -> char {
    s.chars().next().unwrap_or('\0')
}

fn last_char(s: &str) -> char {
    s.chars().next_back().unwrap_or('\0')
}

/// `[begin[-1], end[0]]`, or None when `interpolation_not_enabled?` (single quotes, `%q`).
fn delimited(open: &str, close: &str) -> Option<Ctx> {
    let delims = (last_char(open), first_char(close));
    if delims.0 == '\'' || delims.1 == '\'' || open.starts_with("%q") {
        return None;
    }
    Some(Ctx { delims: Some(delims), space_ok: false })
}

/// A `%W` array (`%w` and other arrays: interpolation disabled or no delimiter, so skipped).
fn percent_w_upper(array: &Node) -> Option<Ctx> {
    let a = array.as_array_node()?;
    let (open, close) = (a.opening_loc()?, a.closing_loc()?);
    if !open.as_slice().starts_with(b"%W") {
        return None;
    }
    let mut ctx = delimited(text(&open), text(&close))?;
    ctx.space_ok = true;
    Some(ctx)
}

fn heredoc_ctx(opening: &str, str_heredoc: bool) -> Option<Ctx> {
    // heredoc_with_disabled_interpolation?
    if opening.ends_with('\'') {
        return None;
    }
    Some(Ctx { delims: None, space_ok: str_heredoc })
}

/// The context of a Prism StringNode as parser `on_str` sees it, plus the byte range scanned
/// (`str_contents_range`); None when nothing in it is checked.
fn context(node: &Node, ancestors: &[Node]) -> Option<(Ctx, usize, usize)> {
    let s = node.as_string_node().unwrap();
    let loc = node.location();
    let parent = ancestors.last();
    if let Some(open) = s.opening_loc() {
        let open = text(&open);
        if open.starts_with('?') {
            return None;
        }
        if is_heredoc(node) {
            let (start, end) = helpers::heredoc::body_range(node)?;
            // A multi-line body is a parser dstr whose `str` lines are not `heredoc?`.
            return Some((heredoc_ctx(open, !is_parser_dstr(node))?, start, end));
        }
        let close = s.closing_loc().map_or("", |c| text(&c));
        return Some((delimited(open, close)?, loc.start_offset(), loc.end_offset()));
    }
    // A str without loc.begin: a %W element, or a piece of an interpolated literal.
    let parent = parent?;
    let ctx = match parent {
        Node::ArrayNode { .. } => percent_w_upper(parent)?,
        Node::InterpolatedStringNode { .. } => {
            let p = parent.as_interpolated_string_node().unwrap();
            match p.opening_loc() {
                Some(o) if is_heredoc(parent) => heredoc_ctx(text(&o), false)?,
                Some(o) => delimited(text(&o), p.closing_loc().map_or("", |c| text(&c)))?,
                // A %W element with interpolation; a concatenation has no delimiter at all.
                None => percent_w_upper(&ancestors[ancestors.len().checked_sub(2)?])?,
            }
        }
        _ => return None,
    };
    Some((ctx, loc.start_offset(), loc.end_offset()))
}

impl Cop for RedundantStringEscape {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some((ctx, start, end)) = context(node, cx.ancestors) else { return };
        let text = cx.src.text;
        let body = &text[start..end];
        let mut i = 0;
        while let Some(off) = body[i..].find('\\') {
            let b = start + i + off;
            // /(\\.)/: `.` is any character but a newline.
            let mut after = text[b + 1..].chars();
            let Some(c1) = after.next().filter(|&c| c != '\n' && b + 1 < end) else {
                i += off + 1;
                continue;
            };
            let e = b + 1 + c1.len_utf8();
            i = e - start;
            if e > end {
                break;
            }
            let c2 = after.next();
            let c3 = after.next();
            if c1 == '\\' || c1.is_alphanumeric() {
                continue;
            }
            if c1 == ' ' && ctx.space_ok {
                continue;
            }
            // disabling_interpolation?
            let interp = |c: Option<char>| matches!(c, Some('{' | '$' | '@'));
            if c1 == '#' && interp(c2) {
                continue;
            }
            let mut before = text[..b].chars().rev();
            let (m1, m2) = (before.next(), before.next());
            if m1 == Some('#') && m2.is_some_and(|c| c != '\\') && matches!(c1, '{' | '$' | '@') {
                continue;
            }
            if c1 == '#' && c2 == Some('\\') && c3 == Some('{') {
                continue;
            }
            if ctx.delims.is_some_and(|(o, c)| c1 == o || c1 == c) {
                continue;
            }
            let msg = format!("Redundant escape of {c1} inside string literal.");
            cx.add_fix(b, e, msg, |c| c.remove(b, b + 1));
        }
    }
}
