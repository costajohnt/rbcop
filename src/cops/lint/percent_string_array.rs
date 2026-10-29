use crate::helpers::percent_literal::percent_type;
use crate::prelude::*;

pub const NAME: &str = "Lint/PercentStringArray";
pub static COP: PercentStringArray = PercentStringArray;
pub struct PercentStringArray;

// Ruby's `^`/`$` are line anchors.
static QUOTES_AND_COMMAS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
    [Regex::new(r"(?m),$").unwrap(), Regex::new(r"(?m)^'.*'$").unwrap(), Regex::new(r#"(?m)^".*"$"#).unwrap()]
});
static LEADING_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^['"]"#).unwrap());
static TRAILING_QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)['"]?,?$"#).unwrap());

const MSG: &str = "Within `%w`/`%W`, quotes and ',' are unnecessary and may be unwanted in the resulting strings.";
const MSG_SPLIT_STRING: &str = "Within `%w`/`%W`, quoted strings are split at whitespace; use an array literal instead.";

fn is_quote(c: char) -> bool {
    c == '\'' || c == '"'
}

/// split_quoted_string?: a value opens a quote that a later value closes.
fn split_quoted_string(elements: &[Node]) -> bool {
    let sources: Vec<&str> = elements.iter().map(|e| node_text(e).strip_suffix(',').unwrap_or(node_text(e))).collect();
    sources.iter().enumerate().any(|(i, source)| {
        let Some(quote) = unclosed_quote(source) else { return false };
        sources[i + 1..].iter().any(|rest| closing_quote(rest, quote))
    })
}

/// unclosed_quote: the quote a value starts with but does not end with.
fn unclosed_quote(source: &str) -> Option<char> {
    let quote = source.chars().next().filter(|&c| is_quote(c))?;
    (source.chars().count() > 1 && !source.ends_with(quote)).then_some(quote)
}

fn closing_quote(source: &str, quote: char) -> bool {
    source.ends_with(quote) && (source.chars().count() == 1 || !source.starts_with(quote))
}

/// array_literal_source: `[content]` when it parses (no comments) to an array of quoted strings.
fn array_literal_source(content: &str, cx: &Cx) -> Option<String> {
    let literal = format!("[{content}]");
    let mut buf = Vec::new();
    let result = helpers::reparsed_equivalence::parse_clean(&literal, &mut buf, cx.src.version, cx.src.legacy())?;
    if result.comments().next().is_some() {
        return None;
    }
    let program = result.node();
    let stmts: Vec<Node> = program.as_program_node().unwrap().statements().body().iter().collect();
    let [array] = stmts.as_slice() else { return None };
    // quoted_string?: a str or dstr with a `begin` (not a heredoc).
    let strings = array.as_array_node()?.elements().iter().all(|e| {
        let open = match (e.as_string_node(), e.as_interpolated_string_node()) {
            (Some(s), _) => s.opening_loc(),
            (_, Some(d)) => d.opening_loc(),
            _ => None,
        };
        open.is_some() && !is_heredoc(&e)
    });
    strings.then(|| literal.clone())
}

impl Cop for PercentStringArray {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        if !matches!(percent_type(&array), Some("%w" | "%W")) {
            return;
        }
        let elements: Vec<Node> = array.elements().iter().collect();
        let l = node.location();
        let (start, end) = (l.start_offset(), l.end_offset());
        if split_quoted_string(&elements) {
            // register_split_quoted_string: the array literal, when it reads as one of strings.
            let literal = match (array.opening_loc(), array.closing_loc()) {
                (Some(o), Some(c)) => array_literal_source(cx.src.slice(o.end_offset(), c.start_offset()), cx),
                _ => None,
            };
            cx.add_fix(start, end, MSG_SPLIT_STRING, |c| {
                if let Some(literal) = &literal {
                    c.replace(start, end, literal);
                }
            });
            return;
        }
        // `value.children.first.to_s`: an interpolated value's first child prints as an
        // s-expression, which never matches.
        let hit = elements.iter().any(|e| {
            let Some(s) = e.as_string_node() else { return false };
            let literal = String::from_utf8_lossy(s.unescaped()).into_owned();
            literal.chars().any(char::is_alphanumeric) && QUOTES_AND_COMMAS.iter().any(|p| p.is_match(&literal))
        });
        if !hit {
            return;
        }
        let ranges: Vec<(usize, usize)> =
            elements.iter().map(|e| (e.location().start_offset(), e.location().end_offset())).collect();
        let src = cx.src;
        cx.add_fix(start, end, MSG, |c| {
            for &(start, end) in &ranges {
                let source = src.slice(start, end);
                if let Some(m) = TRAILING_QUOTE.find(source) {
                    c.remove_trailing(start, end, m.len());
                }
                if LEADING_QUOTE.is_match(source) {
                    c.remove_leading(start, end, 1);
                }
            }
        });
    }
}
