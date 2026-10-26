use crate::prelude::*;
use helpers::regexp_parser::{advance_chars, literal, parse, Parsed};
use ruby_prism::Visit;

pub const NAME: &str = "Lint/UnescapedBracketInRegexp";
pub static COP: UnescapedBracketInRegexp = UnescapedBracketInRegexp;
pub struct UnescapedBracketInRegexp;

const MSG: &str = "Regular expression has `]` without escape.";

/// `detect_offenses_in_tree`: byte offsets (in the pattern) of the literal `]`s to report. An
/// empty set (`[]`, which regexp_parser reads where Onigmo sees a literal `]`) skips the next one.
fn offenses(parsed: &Parsed) -> Vec<usize> {
    let mut events: Vec<(usize, bool)> = parsed.sets.iter().filter(|s| s.members.is_empty()).map(|s| (s.start, true)).collect();
    events.extend(parsed.literal_brackets.iter().map(|&b| (b, false)));
    events.sort();
    let mut skip = false;
    let mut out = vec![];
    for (pos, empty_set) in events {
        if empty_set {
            skip = true;
        } else if skip {
            skip = false;
        } else if pos != 0 {
            out.push(pos);
        }
    }
    out
}

fn report(cx: &mut Cx, start: usize) {
    let end = advance_chars(cx.src.text, start, 1);
    cx.add_fix(start, end, MSG, |c| c.replace(start, end, "\\]"));
}

/// `each_descendant(:dstr).any?`.
#[derive(Default)]
struct HasDstr(bool);

impl<'pr> Visit<'pr> for HasDstr {
    fn visit_interpolated_string_node(&mut self, node: &ruby_prism::InterpolatedStringNode<'pr>) {
        self.0 |= is_parser_dstr(&node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_string_node(&mut self, node: &ruby_prism::StringNode<'pr>) {
        self.0 |= is_parser_dstr(&node.as_node());
    }
}

/// `(const {nil? cbase} :Regexp)`.
fn regexp_const(node: &Node) -> bool {
    match kind::of(node) {
        kind::CONSTANT_READ_NODE => node.as_constant_read_node().unwrap().name().as_slice() == b"Regexp",
        kind::CONSTANT_PATH_NODE => {
            let p = node.as_constant_path_node().unwrap();
            p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Regexp")
        }
        _ => false,
    }
}

impl Cop for UnescapedBracketInRegexp {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::REGULAR_EXPRESSION_NODE,
            kind::INTERPOLATED_REGULAR_EXPRESSION_NODE,
            kind::MATCH_LAST_LINE_NODE,
            kind::INTERPOLATED_MATCH_LAST_LINE_NODE,
            kind::CALL_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src.text;
        let Some(call) = node.as_call_node() else {
            let Some(lit) = literal(node) else { return };
            let Some(parsed) = parse(&lit.pattern, lit.extended) else { return };
            for pos in offenses(&parsed) {
                report(cx, lit.source_offset(src, pos));
            }
            return;
        };
        if !matches!(method_name(&call), "new" | "compile") || is_csend(&call) || !call.receiver().is_some_and(|r| regexp_const(&r)) {
            return;
        }
        let mut dstr = HasDstr::default();
        if let Some(r) = call.receiver() {
            dstr.visit(&r);
        }
        if let Some(a) = call.arguments() {
            dstr.visit_arguments_node(&a);
        }
        if let Some(b) = call.block() {
            dstr.visit(&b);
        }
        if dstr.0 {
            return;
        }
        let args = call_args(&call);
        let Some(text) = args.first().and_then(|a| a.as_string_node()) else { return };
        // verbatim_string?: offsets into the string's value only point at the right place in the
        // source when the string has no escape sequences, so anything else is left alone (a
        // heredoc, `__FILE__` or `?a` has no `loc.begin`/`loc.end`).
        let Some(open) = text.opening_loc().filter(|o| !o.as_slice().starts_with(b"<<")) else { return };
        let Some(close) = text.closing_loc() else { return };
        if &src.as_bytes()[open.end_offset()..close.start_offset()] != text.unescaped() {
            return;
        }
        let value = &src[open.end_offset()..close.start_offset()];
        let Some(parsed) = parse(value, false) else { return };
        let from = open.end_offset();
        // escaped_bracket: a double-quoted string turns `\]` into `]`, so its backslash needs escaping too.
        let fix = if matches!(open.as_slice(), b"'" | [b'%', b'q', _]) { "\\]" } else { "\\\\]" };
        for pos in offenses(&parsed) {
            let start = from + pos;
            cx.add_fix(start, start + 1, MSG, |c| c.replace(start, start + 1, fix));
        }
    }
}
