//! Pieces of RuboCop's LineLengthHelp mixin shared by Layout/LineLength and StatementModifier.

use regex::Regex;
use std::sync::{Arc, LazyLock, Mutex};

static BRACED_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[\s\S]*\}$").unwrap());

/// `URI::RFC2396_PARSER.make_regexp(schemes)` behind RuboCop's exact-case scheme lookahead
/// (`/(?=#{Regexp.union(schemes)}:)#{regexp}/`), as used by `String#scan`.
pub struct UriRegex {
    schemes: Vec<String>,
    /// X_ABS_URI anchored at the start of the haystack (the lookaheads are checked by hand).
    body: Regex,
}

/// RFC2396_Parser#initialize_pattern's X_ABS_URI, with `\d` and `\h` spelled as ASCII classes.
/// Its `(?!//)` is dropped: the alternative before it always matches when `//` follows (all its
/// parts are optional), and leftmost-first matching never backtracks into the second one.
fn x_abs_uri() -> String {
    let escaped = "%[a-fA-F0-9]{2}";
    let unreserved = r"\-_.!~*'()a-zA-Z0-9";
    let reserved = r";/?:@&=+$,\[\]";
    let uric = format!("(?:[{unreserved}{reserved}]|{escaped})");
    let uric_no_slash = format!("(?:[{unreserved};?:@&=+$,]|{escaped})");
    let hostname = "(?:[a-zA-Z0-9\\-.]|%[0-9a-fA-F]{2})+";
    let ipv4 = r"[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}";
    let hex4 = "[a-fA-F0-9]{1,4}";
    let lastpart = format!("(?:{hex4}|{ipv4})");
    let hexseq1 = format!("(?:{hex4}:)*{hex4}");
    let hexseq2 = format!("(?:{hex4}:)*{lastpart}");
    let ipv6 = format!("(?:{hexseq2}|(?:{hexseq1})?::(?:{hexseq2})?)");
    let host = format!(r"(?:{hostname}|{ipv4}|\[{ipv6}\])");
    let userinfo = format!("(?:[{unreserved};:&=+$,]|{escaped})*");
    let pchar = format!("(?:[{unreserved}:@&=+$,]|{escaped})");
    let segment = format!("{pchar}*(?:;{pchar}*)*");
    let abs_path = format!("/{segment}(?:/{segment})*");
    let reg_name = format!("(?:[{unreserved}$,;:@&=+]|{escaped})+");
    let opaque = format!("{uric_no_slash}{uric}*");
    format!(
        r"\A[a-zA-Z][\-+.a-zA-Z0-9]*:(?:{opaque}|(?:(?://(?:(?:(?:{userinfo}@)?(?:{host})(?::[0-9]*)?)?|{reg_name}))?(?:{abs_path})?)(?:\?{uric}*)?)(?:\#{uric}*)?"
    )
}

impl UriRegex {
    /// RuboCop's lookahead at `rest`: a scheme in its exact case, then `:` (it implies
    /// make_regexp's case-insensitive one).
    fn starts_uri(&self, rest: &str) -> bool {
        self.schemes.iter().any(|s| rest.starts_with(s.as_str()) && rest[s.len()..].starts_with(':'))
    }

    /// Byte ranges of `line.scan(uri_regexp)`.
    pub fn scan(&self, line: &str) -> Vec<(usize, usize)> {
        let mut out = vec![];
        let mut pos = 0;
        while let Some(p) = line[pos..].char_indices().map(|(i, _)| pos + i).find(|&p| self.starts_uri(&line[p..])) {
            let Some(m) = self.body.find(&line[p..]) else { break };
            out.push((p, p + m.end()));
            pos = p + m.end().max(1);
        }
        out
    }
}

/// The URI regexp for these schemes, compiled once per scheme list.
pub fn uri_regex(schemes: &[String]) -> Option<Arc<UriRegex>> {
    static BODY: LazyLock<Regex> = LazyLock::new(|| Regex::new(&x_abs_uri()).unwrap());
    static CACHE: Mutex<Vec<Arc<UriRegex>>> = Mutex::new(Vec::new());
    if schemes.is_empty() {
        return None;
    }
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = cache.iter().find(|r| r.schemes == schemes) {
        return Some(r.clone());
    }
    let r = Arc::new(UriRegex { schemes: schemes.to_vec(), body: BODY.clone() });
    cache.push(r.clone());
    Some(r)
}

/// URI::RFC3986_Parser::RFC3986_URI (what `URI.parse` matches a string starting with a scheme
/// against), with subroutine calls inlined and possessive quantifiers made greedy (no difference
/// here: each is followed by a character it cannot match). `(?!/)` becomes a slash-free first
/// character.
fn rfc3986_uri() -> String {
    let h = "[0-9A-Fa-f]";
    let dec = "(?:[1-9][0-9]|1[0-9]{2}|2[0-4][0-9]|25[0-5]|[0-9])";
    let ipv4 = format!(r"{dec}\.{dec}\.{dec}\.{dec}");
    let h16 = format!("{h}{{1,4}}");
    let ls32 = format!("(?:{h16}:{h16}|{ipv4})");
    let ipv6 = [
        format!("(?:{h16}:){{6}}{ls32}"),
        format!("::(?:{h16}:){{5}}{ls32}"),
        format!("{h16}::(?:{h16}:){{4}}{ls32}"),
        format!("(?:(?:{h16}:)?{h16})?::(?:{h16}:){{3}}{ls32}"),
        format!("(?:(?:{h16}:){{0,2}}{h16})?::(?:{h16}:){{2}}{ls32}"),
        format!("(?:(?:{h16}:){{0,3}}{h16})?::{h16}:{ls32}"),
        format!("(?:(?:{h16}:){{0,4}}{h16})?::{ls32}"),
        format!("(?:(?:{h16}:){{0,5}}{h16})?::{h16}"),
        format!("(?:(?:{h16}:){{0,6}}{h16})?::"),
    ]
    .join("|");
    let future = format!(r"v{h}+\.[!$&-.0-9:;=A-Z_a-z~]+");
    let host = format!(r"(?:\[(?:{ipv6}|{future})\]|{ipv4}|(?:%{h}{h}|[!$&-.0-9;=A-Z_a-z~])*)");
    let userinfo = format!("(?:%{h}{h}|[!$&-.0-9:;=A-Z_a-z~])*");
    let seg = format!("(?:%{h}{h}|[!$&-.0-9:;=@A-Z_a-z~/])");
    let seg_ns = format!("(?:%{h}{h}|[!$&-.0-9:;=@A-Z_a-z~])");
    let fragment = format!("(?:%{h}{h}|[!$&-.0-9:;=@A-Z_a-z~/?])*");
    format!(
        r"\A[A-Za-z][+\-.0-9A-Za-z]*:(?://(?:{userinfo}@)?{host}(?::[0-9]*)?(?:/{seg}*)?|/(?:{seg_ns}{seg}*)?|{seg_ns}{seg}*|)(?:\?[^#]*)?(?:\#{fragment})?\z"
    )
}

/// Would `URI.parse` accept this (`valid_uri?`)? Non-ASCII is rejected before matching.
fn valid_uri(s: &str) -> bool {
    static RFC3986: LazyLock<Regex> = LazyLock::new(|| Regex::new(&rfc3986_uri()).unwrap());
    s.is_ascii() && RFC3986.is_match(s)
}

/// The last valid URI in `line` (`match_uris(line).last`).
pub fn last_uri(uri: &UriRegex, line: &str) -> Option<(usize, usize)> {
    uri.scan(line).into_iter().rfind(|&(s, e)| valid_uri(&line[s..e]))
}

/// The last match of `pattern` (`match_qualified_names(line).last`).
pub fn last_match(pattern: &Regex, line: &str) -> Option<(usize, usize)> {
    pattern.find_iter(line).last().map(|m| (m.start(), m.end()))
}

/// `find_excessive_range`, in character columns: the last match (`last_uri` / `last_match`),
/// extended to the next whitespace (or past the line's closing `}`), when it starts or ends at or
/// beyond `max`. `diff` is the line's tab indentation difference.
pub fn find_excessive_range(line: &str, last: Option<(usize, usize)>, max: usize, diff: usize) -> Option<(usize, usize)> {
    let (start, mut end) = last?;
    if BRACED_LINE.is_match(line)
        && let Some(close) = line[end..].rfind('}')
    {
        end += close + 1;
    }
    end += line[end..].bytes().position(crate::helpers::parser_tokens::is_ruby_space).unwrap_or(line.len() - end);
    let col = |b: usize| line[..b].chars().count() + diff;
    let (b, e) = (col(start), col(end));
    (b >= max || e >= max).then_some((b, e))
}

#[cfg(test)]
mod tests {
    /// Matches `line.scan(uri_regexp)` with RuboCop's exact-case lookahead on
    /// `URI::RFC2396_PARSER.make_regexp(%w[http https])` (Ruby 4.0).
    #[test]
    fn uri_regex_is_leftmost_first() {
        let re = super::uri_regex(&["http".into(), "https".into()]).unwrap();
        let all = |s: &str| re.scan(s).into_iter().map(|(a, b)| s[a..b].to_string()).collect::<Vec<_>>();
        assert_eq!(all("x('http://').y('https://a.b/c?d=[1]#e')"), ["http://", "https://a.b/c?d=[1]#e')"]);
        assert_eq!(all("%r{https://.*github.+/x}"), ["https://."]);
        assert!(all("HTTP:foo;bar").is_empty());
        assert!(all("httpS://x").is_empty());
    }
}
