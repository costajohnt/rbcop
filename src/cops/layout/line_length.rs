use crate::directives::DIRECTIVE;
use crate::prelude::*;

pub const NAME: &str = "Layout/LineLength";
pub static COP: LineLength = LineLength;
pub struct LineLength;

static QUALIFIED_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:[A-Z][A-Za-z0-9_]*::)+[A-Za-z_][A-Za-z0-9_]*\b").unwrap());
static BRACED_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[\s\S]*\}$").unwrap());
static RBS_ANNOTATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:#:|#\[.+\]|#\|)").unwrap());

/// URI::RFC2396_PARSER.make_regexp(schemes), approximated: a bare `scheme:` matches, and
/// brackets are allowed in the query and fragment but end the path.
fn uri_regex(schemes: &[String]) -> Option<Regex> {
    if schemes.is_empty() {
        return None;
    }
    let alts: Vec<String> = schemes.iter().map(|s| regex::escape(s)).collect();
    let uric = r"[\-_.!~*'()a-zA-Z\d;/?:@&=+$,\[\]%]";
    let path = r"[\-_.!~*'()a-zA-Z\d:@&=+$,;/%]*";
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

struct Settings {
    max: usize,
    tab_width: usize,
}

impl Settings {
    fn indentation_difference(&self, line: &str) -> usize {
        if self.tab_width == 0 {
            return 0;
        }
        line.bytes().take_while(|&b| b == b'\t').count() * (self.tab_width - 1)
    }

    fn line_length(&self, line: &str) -> usize {
        line.chars().count() + self.indentation_difference(line)
    }

    /// LineLengthHelp#find_excessive_range, in character columns.
    fn excessive_range(&self, line: &str, pattern: &Regex, check_uri: bool) -> Option<(usize, usize)> {
        let m = pattern.find_iter(line).filter(|m| !check_uri || valid_uri(m.as_str())).last()?;
        let mut end = m.end();
        if BRACED_LINE.is_match(line)
            && let Some(close) = line[end..].rfind('}')
        {
            end += close + 1;
        }
        end += line[end..].find(char::is_whitespace).unwrap_or(line.len() - end);
        let diff = self.indentation_difference(line);
        let col = |b: usize| line[..b].chars().count() + diff;
        let (b, e) = (col(m.start()), col(end));
        (b >= self.max || e >= self.max).then_some((b, e))
    }
}

impl Cop for LineLength {
    fn on_file(&self, cx: &mut Cx) {
        let cfg = cx.cfg;
        let max = cfg.int("Max").unwrap_or(120).max(0) as usize;
        let indentation_width = cx.config.for_cop("Layout/IndentationStyle").int("IndentationWidth");
        let width = indentation_width.or_else(|| cx.config.for_cop("Layout/IndentationWidth").int("Width")).unwrap_or(2);
        let s = Settings { max, tab_width: width.max(0) as usize };
        let allow_uri = cfg.bool("AllowURI").unwrap_or(true);
        let allow_qn = cfg.bool("AllowQualifiedName").unwrap_or(true);
        let allow_directives = match cfg.bool("IgnoreCopDirectives") {
            Some(v) => v,
            None => cfg.bool("AllowCopDirectives").unwrap_or(true),
        };
        let allow_rbs = cfg.bool("AllowRBSInlineAnnotation").unwrap_or(false);
        let mut allowed = cfg.regexes("AllowedPatterns");
        allowed.extend(cfg.regexes("IgnoredPatterns"));
        let heredoc_ok = |delim: &str| match cfg.get("AllowHeredoc") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Seq(list)) => list.iter().any(|v| v.as_string().as_deref() == Some(delim)),
            _ => false,
        };
        let uri = if allow_uri { uri_regex(&cfg.strings("URISchemes")) } else { None };
        let src = cx.src;

        for (i, line) in src.lines.iter().enumerate() {
            let lineno = i + 1;
            let len = s.line_length(line);
            if len <= max {
                continue;
            }
            if allowed.iter().any(|r| r.is_match(line))
                || (i == 0 && line.starts_with("#!"))
                || cx.facts.heredocs.iter().any(|h| h.first <= lineno && lineno < h.terminator && heredoc_ok(&h.delimiter))
            {
                continue;
            }
            let comment = src.comment_on_line(lineno).map(|c| src.slice(c.start, c.end));
            if allow_rbs
                && comment.is_some_and(|c| RBS_ANNOTATION.is_match(c))
            {
                continue;
            }
            let start = src.line_start(lineno);
            let at = |col: usize| start + line.char_indices().nth(col).map_or(line.len(), |(b, _)| b);
            if allow_directives && comment.is_some_and(|c| crate::directives::parse(c).is_some()) {
                let before = DIRECTIVE.split(line).next().unwrap_or("").trim_end().chars().count();
                if before > max {
                    cx.add_with(at(max), at(before), format!("Line is too long. [{before}/{max}]"), false);
                }
                continue;
            }
            let mut range = None;
            if allow_uri || allow_qn {
                let u = uri.as_ref().and_then(|r| s.excessive_range(line, r, true));
                let q = if allow_qn { s.excessive_range(line, &QUALIFIED_NAME, false) } else { None };
                let ok = |r: (usize, usize)| r.0 < max && r.1 == len;
                let allowed_combination = match (u, q) {
                    (Some(u), Some(q)) => ok(u) && ok(q),
                    (Some(r), None) | (None, Some(r)) => ok(r),
                    (None, None) => false,
                };
                if allowed_combination {
                    continue;
                }
                range = u.or(q);
            }
            let from = match range {
                Some((b, e)) if b < max => e,
                _ => max.saturating_sub(s.indentation_difference(line)),
            };
            // ponytail: RuboCop marks the offense [Correctable] when CheckLineBreakable finds a break point; not modelled yet.
            cx.add_with(at(from), at(line.chars().count()), format!("Line is too long. [{len}/{max}]"), false);
        }
    }
}
