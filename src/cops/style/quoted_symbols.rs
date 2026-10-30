use crate::helpers::alignment::cop_enabled;
use crate::helpers::percent_array::safe_string_literal;
use crate::helpers::ruby_string::{inspect, inspect_binary};
use crate::helpers::string_help::wrong_quotes;
use crate::prelude::*;

pub const NAME: &str = "Style/QuotedSymbols";
pub static COP: QuotedSymbols = QuotedSymbols;
pub struct QuotedSymbols;

const MSG_SINGLE: &str = "Prefer single-quoted symbols when you don't need string interpolation or special symbols.";
const MSG_DOUBLE: &str = "Prefer double-quoted symbols unless you need single quotes to avoid extra backslashes for escaping.";

/// `/\A:?(['"]).*?\1\z/m`
fn quoted(src: &str) -> bool {
    let s = src.strip_prefix(':').unwrap_or(src);
    match s.as_bytes().first() {
        Some(&q @ (b'\'' | b'"')) => s.len() > 1 && s.ends_with(q as char),
        _ => false,
    }
}

/// `/" | (?<!\\)\\[aAbcdefkMnprsStuUxzZ0-7] | \#[@{$]/x.match?(source)`
fn needs_double(src: &str) -> bool {
    let b = src.as_bytes();
    (0..b.len()).any(|i| match b[i] {
        b'"' => true,
        b'\\' => {
            (i == 0 || b[i - 1] != b'\\')
                && b.get(i + 1).is_some_and(|c| b"aAbcdefkMnprsStuUxzZ01234567".contains(c))
        }
        b'#' => matches!(b.get(i + 1), Some(b'@' | b'{' | b'$')),
        _ => false,
    })
}

fn parser_dsym(sym: &ruby_prism::SymbolNode, target_ruby: f64) -> bool {
    crate::ast::is_parser_dsym(&sym.as_node(), target_ruby)
}

impl Cop for QuotedSymbols {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::SYMBOL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let sym = node.as_symbol_node().unwrap();
        let loc = node.location();
        // A quoted label's parser range (`"a"` in `"a": 1`) excludes the colon.
        let label = sym.closing_loc().is_some_and(|c| c.as_slice().len() == 2 && c.as_slice().ends_with(b":"));
        let (start, end) = (loc.start_offset(), loc.end_offset() - usize::from(label));
        let src = &cx.src.text[start..end];
        if !quoted(src) || parser_dsym(&sym, cx.config.target_ruby) {
            return;
        }
        let mut style = cx.cfg.str("EnforcedStyle").unwrap_or("same_as_string_literals").to_string();
        if style == "same_as_string_literals" {
            style = if cop_enabled(cx, "Style/StringLiterals") {
                cx.config.for_cop("Style/StringLiterals").str("EnforcedStyle").unwrap_or("").to_string()
            } else {
                "single_quotes".to_string()
            };
        }
        let single = style == "single_quotes";
        let msg = if single { MSG_SINGLE } else { MSG_DOUBLE };

        // SymbolHelp#hash_key?: the pair's first child, compared structurally (a value equal to
        // the key counts too).
        let (hash_key, colon) = match cx.parent().and_then(|p| p.as_assoc_node()) {
            Some(pair) => {
                let key = pair.key();
                let is_key = key.location().start_offset() == start
                    || key.as_symbol_node().is_some_and(|k| !parser_dsym(&k, cx.config.target_ruby) && k.unescaped() == sym.unescaped());
                (is_key, pair.operator_loc().is_none())
            }
            None => (false, false),
        };
        let wrong = if hash_key { wrong_quotes(src, single) } else { wrong_quotes(&src[1..], single) };
        let invalid_double = style == "double_quotes" && !needs_double(src);
        if !(wrong || invalid_double) {
            // correct_style_detected with the resolved `style`.
            return cx.style_detected(&[&style]);
        }
        if cx.block_runs(start, end) {
            // alternative_style: `(supported_styles - [style, :same_as_string_literals]).first`
            let other = cx.cfg.strings("SupportedStyles").into_iter().find(|s| *s != style && s != "same_as_string_literals");
            if let Some(other) = other {
                cx.style_detected(&[&other]);
            }
        }
        let binary = cx.src.binary;
        let correct_quotes = |s: &str| {
            if single {
                safe_string_literal(s.as_bytes()).replace("\\\\", "\\").replace("\\\"", "\"")
            } else {
                let s = s.replace("\\'", "'");
                let inspected = if binary { inspect_binary(s.as_bytes()) } else { inspect(s.as_bytes()) };
                inspected.replace("\\\\", "\\")
            }
        };
        // `source[1..-2]` / `source[2..-2]` by characters.
        let chars: Vec<char> = src.chars().collect();
        let slice = |from: usize| -> String {
            if chars.len() < from + 1 { String::new() } else { chars[from..chars.len() - 1].iter().collect() }
        };
        let replacement =
            if hash_key && colon { correct_quotes(&slice(1)) } else { format!(":{}", correct_quotes(&slice(2))) };
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
