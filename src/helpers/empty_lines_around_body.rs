//! RuboCop's EmptyLinesAroundBody mixin (Layout/EmptyLinesAround{Class,Module,Method,Block,Begin}Body
//! and Layout/EmptyLinesAroundExceptionHandlingKeywords).

use crate::cop::Cx;
use crate::node_kind as kind;
use regex::Regex;
use ruby_prism::Node;
use std::sync::LazyLock;

static COMMENT_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t\r\n\x0c\x0b]*#").unwrap());

/// Parser-gem children of a body: a multi-statement body (`begin`) has several, anything else one.
pub fn body_children<'pr>(body: Option<Node<'pr>>) -> Option<Vec<Node<'pr>>> {
    let body = body?;
    Some(match body.as_statements_node() {
        Some(s) => s.body().iter().collect(),
        None => vec![body],
    })
}

/// `empty_line_required?`: `{any_def class module (send nil? {:private :protected :public})}`,
/// returning the parser node type for the deferred message.
fn required_type(node: &Node) -> Option<&'static str> {
    match kind::of(node) {
        kind::DEF_NODE => Some(if node.as_def_node().unwrap().receiver().is_some() { "defs" } else { "def" }),
        kind::CLASS_NODE => Some("class"),
        kind::MODULE_NODE => Some("module"),
        kind::CALL_NODE => {
            let c = node.as_call_node().unwrap();
            let name = c.name().as_slice();
            // No arguments: `private :a` (and a `&blk` pass, a parser argument) is no match; with a
            // `{}`/`do` block the statement is a parser `block`.
            (c.receiver().is_none()
                && c.arguments().is_none()
                && c.block().is_none()
                && matches!(name, b"private" | b"protected" | b"public"))
            .then_some("send")
        }
        _ => None,
    }
}

fn is_constant_definition(node: &Node) -> bool {
    matches!(kind::of(node), kind::CLASS_NODE | kind::MODULE_NODE)
}

/// `check(node, body, adjusted_first_line:)`. `range` is the node's source range, `body` its
/// parser-gem children (None for no body), `kind_name` the cop's KIND.
pub fn check(
    cx: &mut Cx,
    kind_name: &str,
    style: &str,
    range: (usize, usize),
    body: Option<&[Node]>,
    adjusted_first_line: Option<usize>,
) {
    if body.is_none() && style != "no_empty_lines" {
        return;
    }
    let src = cx.src;
    let first = src.line_of(range.0);
    let last = src.line_of(range.1.saturating_sub(1).max(range.0));
    if first == last {
        return;
    }
    let first_line = adjusted_first_line.unwrap_or(first);
    let namespace = body.is_some_and(|b| b.len() == 1 && is_constant_definition(&b[0]));
    match style {
        "empty_lines_except_namespace" => {
            let s = if namespace { "no_empty_lines" } else { "empty_lines" };
            check_both(cx, kind_name, s, first_line, last);
        }
        "empty_lines_special" => {
            let Some(body) = body else { return };
            if namespace {
                check_both(cx, kind_name, "no_empty_lines", first_line, last);
            } else {
                if body.first().is_some_and(|b| required_type(b).is_some()) {
                    check_source(cx, kind_name, "empty_lines", first_line as i64, "beginning");
                } else {
                    check_source(cx, kind_name, "no_empty_lines", first_line as i64, "beginning");
                    check_deferred(cx, body);
                }
                check_source(cx, kind_name, "empty_lines", last as i64 - 2, "end");
            }
        }
        _ => check_both(cx, kind_name, style, first_line, last),
    }
}

fn check_both(cx: &mut Cx, kind_name: &str, style: &str, first_line: usize, last_line: usize) {
    let (b, e) = match style {
        "beginning_only" => ("empty_lines", "no_empty_lines"),
        "ending_only" => ("no_empty_lines", "empty_lines"),
        s => (s, s),
    };
    check_source(cx, kind_name, b, first_line as i64, "beginning");
    check_source(cx, kind_name, e, last_line as i64 - 2, "end");
}

fn check_source(cx: &mut Cx, kind_name: &str, style: &str, line_no: i64, desc: &str) {
    match style {
        "no_empty_lines" => {
            let msg = format!("Extra empty line detected at {kind_name} body {desc}.");
            check_line(cx, style, line_no, &msg, true);
        }
        "empty_lines" => {
            let msg = format!("Empty line missing at {kind_name} body {desc}.");
            check_line(cx, style, line_no, &msg, false);
        }
        _ => {}
    }
}

/// Ruby `lines[i]` with negative indexes counting from the end.
fn line_at<'a>(lines: &[&'a str], i: i64) -> Option<&'a str> {
    let idx = if i < 0 { lines.len() as i64 + i } else { i };
    if idx < 0 {
        return None;
    }
    lines.get(idx as usize).copied()
}

/// `check_line(style, line, msg) { |l| l.empty? == want_empty }`: `line` is a 0-based index into
/// `processed_source.lines`.
pub fn check_line(cx: &mut Cx, style: &str, line: i64, msg: &str, want_empty: bool) {
    let Some(l) = line_at(&cx.src.lines, line) else { return };
    if l.is_empty() != want_empty {
        return;
    }
    let offset = if style == "empty_lines" && msg.contains("end.") { 2 } else { 1 };
    let line_no = line + offset;
    if line_no < 1 || line_no as usize > cx.src.line_count() {
        return;
    }
    let start = cx.src.line_start(line_no as usize);
    if start >= cx.src.text.len() {
        return;
    }
    if style == "empty_lines" {
        cx.add_fix(start, start + 1, msg, |c| c.insert_before(start, start + 1, "\n"));
    } else {
        cx.add_fix(start, start + 1, msg, |c| c.remove(start, start + 1));
    }
}

fn check_deferred(cx: &mut Cx, body: &[Node]) {
    let Some((node, ty)) = body.iter().find_map(|c| required_type(c).map(|t| (c, t))) else { return };
    let src = cx.src;
    let send_line = src.line_of(node.location().start_offset()) as i64;
    let mut line = 0;
    let mut l = send_line - 2;
    while l >= 0 {
        if !line_at(&src.lines, l).is_some_and(|s| COMMENT_LINE.is_match(s)) {
            line = l;
            break;
        }
        l -= 1;
    }
    if line_at(&src.lines, line).is_none_or(str::is_empty) {
        return;
    }
    let line_no = (line + 2) as usize;
    if line_no > src.line_count() {
        return;
    }
    let start = src.line_start(line_no);
    let msg = format!("Empty line missing before first {ty} definition");
    cx.add_fix(start, start + 1, msg, |c| c.insert_before(start, start + 1, "\n"));
}
