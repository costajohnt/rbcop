use crate::helpers::sorbet_sigil::{SIGIL, leading_comments};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/EnforceSigilOrder";
pub static COP: EnforceSigilOrder = EnforceSigilOrder;
pub struct EnforceSigilOrder;

const MSG: &str = "Magic comments should be in the following order: encoding, typed, warn_indent, frozen_string_literal.";

/// PREFERRED_ORDER's keys (Ruby `\s` is ASCII whitespace); SIGIL_REGEX is the second.
static CODING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#[ \t\r\n\x0b\x0c]+(en)?coding:").unwrap());
static INDENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#[ \t\r\n\x0b\x0c]+warn_indent:").unwrap());
static FROZEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#[ \t\r\n\x0b\x0c]+frozen_string_literal:").unwrap());

fn preferred() -> [&'static Regex; 4] {
    [&CODING, &SIGIL, &INDENT, &FROZEN]
}

impl Cop for EnforceSigilOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        if helpers::parser_tokens::tokens_empty(src, cx.facts.first_code) {
            return;
        }
        let text = |&(s, e): &(usize, usize)| &src.text[s..e];
        let tokens: Vec<(usize, usize)> =
            leading_comments(src).into_iter().filter(|t| preferred().iter().any(|re| re.is_match(text(t)))).collect();
        if tokens.is_empty() {
            return;
        }
        // The order in the source (each token's first matching key), and the preferred order of the
        // keys any token matches.
        let mut order: Vec<usize> = Vec::new();
        for t in &tokens {
            if let Some(i) = preferred().iter().position(|re| re.is_match(text(t)))
                && !order.contains(&i)
            {
                order.push(i);
            }
        }
        let expected: Vec<usize> =
            (0..4).filter(|&i| tokens.iter().any(|t| preferred()[i].is_match(text(t)))).collect();
        if order == expected {
            return;
        }
        // autocorrect: the tokens rewritten in the preferred order, then the blank lines between the
        // first and the last removed.
        let sorted: Vec<&str> =
            preferred().iter().flat_map(|re| tokens.iter().filter(|t| re.is_match(text(t))).map(text)).collect();
        let replacements: Vec<(usize, usize, &str)> =
            tokens.iter().zip(&sorted).map(|(&(s, e), &new)| (s, e, new)).collect();
        let lines: Vec<usize> = tokens.iter().map(|&(s, _)| src.line_of(s)).collect();
        let (min, max) = (*lines.iter().min().unwrap(), *lines.iter().max().unwrap());
        let blank: Vec<usize> = (min..max)
            .filter(|l| !lines.contains(l))
            .map(|l| src.line_start(l))
            .filter(|&b| src.text.as_bytes().get(b) == Some(&b'\n'))
            .collect();
        for &(s, e) in &tokens {
            let (replacements, blank) = (&replacements, &blank);
            cx.add_fix(s, e, MSG, |k| {
                for &(b, e, new) in replacements {
                    k.replace(b, e, new);
                }
                for &b in blank {
                    k.remove(b, b + 1);
                }
            });
        }
    }
}
