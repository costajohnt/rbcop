use crate::prelude::*;

pub const NAME: &str = "Style/RegexpLiteral";
pub static COP: RegexpLiteral = RegexpLiteral;
pub struct RegexpLiteral;

/// A regexp literal in parser terms: opening, closing (delimiter plus options in Prism) and the
/// parts as (start, end, is_str): `str` and `begin` children; `#@ivar` parts are left out.
struct Regexp {
    open: (usize, usize),
    close: (usize, usize),
    parts: Vec<(usize, usize, bool)>,
}

fn regexp(node: &Node) -> Option<Regexp> {
    let range = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
    let simple = |open: ruby_prism::Location, content: ruby_prism::Location, close: ruby_prism::Location| {
        let c = range(content);
        Regexp { open: range(open), close: range(close), parts: if c.0 == c.1 { vec![] } else { vec![(c.0, c.1, true)] } }
    };
    let interpolated = |open: ruby_prism::Location, parts: ruby_prism::NodeList, close: ruby_prism::Location| Regexp {
        open: range(open),
        close: range(close),
        parts: parts
            .iter()
            .filter_map(|p| match kind::of(&p) {
                kind::STRING_NODE => Some((range(p.location()).0, range(p.location()).1, true)),
                kind::EMBEDDED_STATEMENTS_NODE => Some((range(p.location()).0, range(p.location()).1, false)),
                _ => None,
            })
            .collect(),
    };
    match kind::of(node) {
        kind::REGULAR_EXPRESSION_NODE => {
            let r = node.as_regular_expression_node().unwrap();
            Some(simple(r.opening_loc(), r.content_loc(), r.closing_loc()))
        }
        kind::MATCH_LAST_LINE_NODE => {
            let r = node.as_match_last_line_node().unwrap();
            Some(simple(r.opening_loc(), r.content_loc(), r.closing_loc()))
        }
        kind::INTERPOLATED_REGULAR_EXPRESSION_NODE => {
            let r = node.as_interpolated_regular_expression_node().unwrap();
            Some(interpolated(r.opening_loc(), r.parts(), r.closing_loc()))
        }
        kind::INTERPOLATED_MATCH_LAST_LINE_NODE => {
            let r = node.as_interpolated_match_last_line_node().unwrap();
            Some(interpolated(r.opening_loc(), r.parts(), r.closing_loc()))
        }
        _ => None,
    }
}

fn balanced(text: &[u8], open: u8, close: u8) -> bool {
    let mut depth = 0i32;
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if c == b'\\' {
            // `\\.`: `.` does not match a newline, so a lone `\` before one is skipped alone.
            i += if text.get(i + 1).is_some_and(|&n| n != b'\n') { 2 } else { 1 };
            continue;
        }
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth < 0 {
                return false;
            }
        }
        i += 1;
    }
    depth == 0
}

impl Cop for RegexpLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::REGULAR_EXPRESSION_NODE,
            kind::INTERPOLATED_REGULAR_EXPRESSION_NODE,
            kind::MATCH_LAST_LINE_NODE,
            kind::INTERPOLATED_MATCH_LAST_LINE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(re) = regexp(node) else { return };
        let src = cx.src.text.as_bytes();
        let open_src = &cx.src.text[re.open.0..re.open.1];
        let slash = open_src == "/";
        let body: Vec<u8> = re.parts.iter().filter(|p| p.2).flat_map(|p| src[p.0..p.1].iter().copied()).collect();
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("slashes");
        let allow_inner_slashes = cx.cfg.bool("AllowInnerSlashes").unwrap_or(false);
        let delims = cx.config.for_cop("Style/PercentLiteralDelimiters");
        let preferred: Vec<char> =
            delims.map("PreferredDelimiters").and_then(|m| m.str("%r")).unwrap_or("{}").chars().collect();
        let (p_open, p_close) = (preferred.first().copied(), preferred.get(1).copied());

        if slash {
            let pair = match (p_open, p_close) {
                (Some('('), Some(')')) => Some((b'(', b')')),
                (Some('['), Some(']')) => Some((b'[', b']')),
                (Some('{'), Some('}')) => Some((b'{', b'}')),
                (Some('<'), Some('>')) => Some((b'<', b'>')),
                _ => None,
            };
            if pair.is_some_and(|(o, c)| !balanced(&body, o, c)) {
                return;
            }
        }
        let contains_disallowed_slash = !allow_inner_slashes && body.contains(&b'/');
        let loc = node.location();
        let multiline = cx.src.text[loc.start_offset()..loc.end_offset()].contains('\n');
        let message = if slash {
            let allowed = (style == "slashes" && !contains_disallowed_slash)
                || (style == "mixed" && !multiline && !contains_disallowed_slash);
            (!allowed).then_some("Use `%r` around regular expression.")
        } else {
            let allowed = (style == "slashes" && contains_disallowed_slash)
                || style == "percent_r"
                || (style == "mixed" && multiline)
                || contains_disallowed_slash
                || self.omit_parentheses_allowed(cx, node, &re);
            (!allowed).then_some("Use `//` around regular expression.")
        };
        let Some(message) = message else { return };

        let (new_open, new_close) = if slash {
            (format!("%r{}", p_open.map(String::from).unwrap_or_default()), p_close.map(String::from).unwrap_or_default())
        } else {
            ("/".to_string(), "/".to_string())
        };
        let before: &[u8] = if open_src == "/" || open_src == "%r/" { b"\\/" } else { b"/" };
        let after = if new_open == "/" || new_open == "%r/" { "\\/" } else { "/" };
        // node_body(include_begin_nodes: true), indexed from the end of the opening delimiter.
        let text: Vec<u8> = re.parts.iter().flat_map(|p| src[p.0..p.1].iter().copied()).collect();
        let base = re.open.1;
        let mut edits = Vec::new();
        let mut i = 0;
        while i + before.len() <= text.len() {
            if &text[i..i + before.len()] == before {
                edits.push((base + i, base + i + before.len()));
            }
            i += 1;
        }
        let (open, close) = (re.open, (re.close.0, re.close.0 + 1));
        cx.add_fix(loc.start_offset(), loc.end_offset(), message, |c| {
            c.replace(open.0, open.1, &new_open);
            c.replace(close.0, close.1, &new_close);
            for &(s, e) in &edits {
                c.replace(s, e, after);
            }
        });
    }
}

impl RegexpLiteral {
    /// allowed_omit_parentheses_with_percent_r_literal?
    fn omit_parentheses_allowed(&self, cx: &Cx, node: &Node, re: &Regexp) -> bool {
        if !parent_is_call(cx.ancestors, node) {
            return false;
        }
        // node.content: the `str` children joined.
        let first = re.parts.iter().find(|p| p.2).map(|p| cx.src.text.as_bytes()[p.0]);
        if matches!(first, Some(b' ' | b'=')) {
            return true;
        }
        cx.config.for_cop("Style/MethodCallWithArgsParentheses").str("EnforcedStyle") == Some("omit_parentheses")
    }
}

/// `node.parent&.call_type?` in parser terms: a send/csend parent (not a block it is the body of).
/// A static regexp receiver of `=~` is a `match_with_lvasgn` in parser.
fn parent_is_call(ancestors: &[Node], node: &Node) -> bool {
    if kind::of(node) == kind::MATCH_LAST_LINE_NODE || kind::of(node) == kind::INTERPOLATED_MATCH_LAST_LINE_NODE {
        return false;
    }
    for i in (0..ancestors.len()).rev() {
        let a = &ancestors[i];
        let f = frame(a);
        if f == Frame::Transparent {
            continue;
        }
        if f == Frame::Call && ancestors.get(i + 1).is_some_and(|n| frame(n) == Frame::Block) {
            // We are inside its block: the parser parent is the block.
            return false;
        }
        if let Some(call) = a.as_call_node() {
            let loc = node.location();
            let is_receiver = call
                .receiver()
                .is_some_and(|r| r.location().start_offset() == loc.start_offset() && r.location().end_offset() == loc.end_offset());
            if is_receiver && method_name(&call) == "=~" && kind::of(node) == kind::REGULAR_EXPRESSION_NODE {
                return false;
            }
            return true;
        }
        return false;
    }
    false
}
