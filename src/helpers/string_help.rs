//! RuboCop's StringHelp and StringLiteralsHelp mixins and StringLiteralCorrector
//! (Style/StringLiterals, Style/StringLiteralsInInterpolation, Style/CharacterLiteral).

use crate::cop::Corrector;
use crate::helpers::percent_array::{double_quotes_required, safe_string_literal};
use crate::helpers::ruby_string::{inspect, inspect_binary};
use crate::prelude::*;

/// `on_str` with `node.loc?(:begin)`: a single-line delimited parser `str` (not a heredoc,
/// not a multi-line literal, which parser makes a `dstr`).
pub fn delimited_str<'pr>(node: &Node<'pr>) -> Option<ruby_prism::StringNode<'pr>> {
    let s = node.as_string_node()?;
    (s.opening_loc().is_some() && !is_heredoc(node) && !is_parser_dstr(node)).then_some(s)
}

/// StringHelp#on_regexp ignores the node: a str anywhere inside a regexp is skipped.
pub fn in_regexp(ancestors: &[Node]) -> bool {
    ancestors.iter().any(|a| {
        matches!(a, Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. })
    })
}

/// StringHelp#inside_interpolation?: a parser `begin` ancestor with a dstr/dsym/regexp above it.
pub fn inside_interpolation(ancestors: &[Node]) -> bool {
    let Some(outer) = ancestors.iter().position(|a| {
        matches!(
            a,
            Node::InterpolatedStringNode { .. }
                | Node::InterpolatedSymbolNode { .. }
                | Node::RegularExpressionNode { .. }
                | Node::InterpolatedRegularExpressionNode { .. }
        )
    }) else {
        return false;
    };
    // Below a dstr/dsym/regexp, any path to a str goes through an interpolation `begin`.
    ancestors[outer + 1..].iter().any(|a| a.as_embedded_statements_node().is_some())
}

/// StringLiteralsHelp#wrong_quotes? on a literal's source.
pub fn wrong_quotes(src: &str, single_quotes: bool) -> bool {
    if src.starts_with('%') || src.starts_with('?') {
        return false;
    }
    if single_quotes {
        return !double_quotes_required(src);
    }
    // !/" | \\[^'\\] | \#[@{$]/x.match?(src)
    let b = src.as_bytes();
    let hit = (0..b.len()).any(|i| match b[i] {
        b'"' => true,
        b'\\' => src[i + 1..].chars().next().is_some_and(|c| c != '\'' && c != '\\'),
        b'#' => matches!(b.get(i + 1), Some(b'@' | b'{' | b'$')),
        _ => false,
    });
    !hit
}

/// StringLiteralCorrector.correct for a parser `str`; `binary` for an ASCII-8BIT source, whose
/// strings inspect every non-ASCII byte as `\xNN`.
pub fn correct(c: &mut Corrector, node: &ruby_prism::StringNode, single_quotes: bool, binary: bool) {
    let loc = node.location();
    let content = node.unescaped();
    let text = match (single_quotes, binary) {
        (true, false) => safe_string_literal(content),
        (false, false) => inspect(content),
        (false, true) => inspect_binary(content),
        (true, true) => {
            // Util#to_string_literal with the binary inspect; invalid UTF-8 stays escaped (see
            // percent_array::safe_string_literal).
            let i = inspect_binary(content);
            let escaped = i[1..i.len() - 1].replace("\\\"", "\"");
            match std::str::from_utf8(content) {
                Ok(s) if !double_quotes_required(&escaped) => {
                    format!("'{}'", s.replace('\\', "\\\\").replace("\\\"", "\""))
                }
                _ => i,
            }
        }
    };
    c.replace(loc.start_offset(), loc.end_offset(), &text);
}
