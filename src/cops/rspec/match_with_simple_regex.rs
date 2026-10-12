use crate::helpers::regexp_parser::{self, ExprKind};
use crate::prelude::*;

pub const NAME: &str = "RSpec/MatchWithSimpleRegex";
pub static COP: MatchWithSimpleRegex = MatchWithSimpleRegex;
pub struct MatchWithSimpleRegex;

/// Escapes that are not regexp_parser's EscapeSequence::Literal (`\.` and the like).
const SPECIAL_ESCAPES: &[u8] = b"aefnrtv01234567xucCMdDhHsSwWpPkgAzZbBGRXK";

/// simple_regexp? and regexp_to_string: the string a literal-only pattern matches, or None.
fn simple_string(content: &str) -> Option<String> {
    let exprs = regexp_parser::tree(content, false)?;
    let mut out = String::new();
    for e in &exprs {
        if e.quantifier.is_some() {
            return None;
        }
        let text = &content[e.start..e.end];
        match e.kind {
            ExprKind::Literal => out.push_str(text),
            ExprKind::Terminal if text.starts_with('\\') => {
                let c = text[1..].chars().next()?;
                if c.is_ascii() && SPECIAL_ESCAPES.contains(&(c as u8)) {
                    return None;
                }
                out.push(c);
            }
            _ => return None,
        }
    }
    Some(out)
}

impl Cop for MatchWithSimpleRegex {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :match $regexp)
        let Some(call) = helpers::rspec::nil_send(node, "match") else { return };
        let [arg] = &send_args(&call)[..] else { return };
        let Some(re) = arg.as_regular_expression_node() else { return };
        // regopt children: any flag
        let closing = re.closing_loc().as_slice();
        if closing.len() > 1 {
            return;
        }
        let Some(string) = simple_string(text(&re.content_loc())) else { return };
        // to_string_literal: single quotes unless the text has one
        let double = string.contains('\'');
        let literal = if double { format!("\"{}\"", string.replace('"', "\\\"")) } else { format!("'{string}'") };
        let msg = format!("Prefer using `include({literal})` when the regex is a simple string literal.");
        let (s, e) = send_range(&call);
        // The text goes into a new literal unescaped: withheld unless inert there.
        let inert = !string.contains('\\') && !(double && ["#{", "#@", "#$"].iter().any(|i| string.contains(i)));
        if !inert {
            cx.add_withheld_fix(s, e, msg);
            return;
        }
        let replacement = format!("include({literal})");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
