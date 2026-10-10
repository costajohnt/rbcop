//! Pieces of RuboCop's LineLengthHelp mixin shared by Layout/LineLength and StatementModifier.

use regex::Regex;
use std::sync::LazyLock;

static BRACED_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[\s\S]*\}$").unwrap());

/// URI::RFC2396_PARSER.make_regexp(schemes), approximated: a bare `scheme:` matches, and
/// brackets are allowed in the query and fragment but end the path.
pub fn uri_regex(schemes: &[String]) -> Option<Regex> {
    if schemes.is_empty() {
        return None;
    }
    let alts: Vec<String> = schemes.iter().map(|s| regex::escape(s)).collect();
    let uric = r"[\-_.!~*'()a-zA-Z0-9;/?:@&=+$,\[\]%]";
    let path = r"[\-_.!~*'()a-zA-Z0-9:@&=+$,;/%]*";
    Regex::new(&format!(r"(?i)(?:{}):{path}(?:\?{uric}*)?(?:#{uric}*)?", alts.join("|"))).ok()
}

/// Would `URI.parse` (RFC 3986) accept this? Brackets are only legal around an IPv6 host,
/// `#` may appear once and `%` must start an escape.
// ponytail: covers the failures seen in real code, not the whole RFC 3986 grammar.
fn valid_uri(s: &str) -> bool {
    let b = s.as_bytes();
    !s.contains(['[', ']'])
        && s.matches('#').count() <= 1
        && b.iter().enumerate().all(|(i, &c)| c != b'%' || b.get(i + 1..i + 3).is_some_and(|h| h.iter().all(u8::is_ascii_hexdigit)))
}

/// `find_excessive_range`, in character columns: the last match of `pattern` (a valid URI when
/// `check_uri`), extended to the next whitespace (or past the line's closing `}`), when it starts or
/// ends at or beyond `max`. `diff` is the line's tab indentation difference.
pub fn find_excessive_range(line: &str, pattern: &Regex, check_uri: bool, max: usize, diff: usize) -> Option<(usize, usize)> {
    let m = pattern.find_iter(line).filter(|m| !check_uri || valid_uri(m.as_str())).last()?;
    let mut end = m.end();
    if BRACED_LINE.is_match(line)
        && let Some(close) = line[end..].rfind('}')
    {
        end += close + 1;
    }
    end += line[end..].find(char::is_whitespace).unwrap_or(line.len() - end);
    let col = |b: usize| line[..b].chars().count() + diff;
    let (b, e) = (col(m.start()), col(end));
    (b >= max || e >= max).then_some((b, e))
}
