//! RuboCop's `Cop::Utils::FormatString`: format sequences (`%s`, `%<a>d`, `%{a}`) in a string.

use regex::Regex;
use std::sync::LazyLock;

// Ruby's SEQUENCE uses a lookbehind (`(?<!#)\{name\}`) that the regex crate lacks: the template
// branch is spelled out so that the `{` is never directly preceded by a `#` flag.
static SEQUENCE: LazyLock<Regex> = LazyLock::new(|| {
    let digit_dollar = r"[0-9]+\$";
    let interpolation = r"\#\{.*?\}";
    let flag = format!(r"(?:[ #0+\-]|{digit_dollar})");
    let number = format!(r"(?:[0-9]+|\*(?:{digit_dollar})?|{interpolation})");
    let width = number.clone();
    let precision = format!(r"\.(?:{number})?");
    let ty = r"(?P<type>[bBdiouxXeEfgGaAcps])";
    let name = |g: &str| format!(r"<(?P<{g}>[A-Za-z0-9_]+)>");
    let (n1, n2, n3) = (name("n1"), name("n2"), name("n3"));
    let typed = format!(
        r"{flag}*(?:(?:{width})?(?:{precision})?(?:{n1})?|(?:{width})?{n2}(?:{precision})?|{n3}{flag}*(?:{width})?(?:{precision})?){ty}"
    );
    let template = format!(
        r"(?:{flag}*(?:{width}(?:{precision})?|{precision})|(?:{flag}*(?:[ 0+\-]|{digit_dollar}))?)\{{(?P<tn>[A-Za-z0-9_]+)\}}"
    );
    Regex::new(&format!(r"%(?P<pct>%)|%(?:{typed}|{template})")).unwrap()
});

static DIGIT_DOLLAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([0-9]+)\$").unwrap());

pub struct FormatSequence<'a> {
    pub source: &'a str,
    pub begin: usize,
    pub end: usize,
    pub name: Option<&'a str>,
    pub percent: bool,
}

impl FormatSequence<'_> {
    pub fn arity(&self) -> usize {
        self.source.matches('*').count() + 1
    }

    pub fn max_digit_dollar_num(&self) -> Option<u64> {
        DIGIT_DOLLAR.captures_iter(self.source).map(|c| c[1].parse().unwrap_or(u64::MAX)).max()
    }
}

pub fn format_sequences(s: &str) -> Vec<FormatSequence<'_>> {
    SEQUENCE
        .captures_iter(s)
        .map(|c| {
            let m = c.get(0).unwrap();
            FormatSequence {
                source: m.as_str(),
                begin: m.start(),
                end: m.end(),
                name: ["n1", "n2", "n3", "tn"].iter().find_map(|g| c.name(g)).map(|g| g.as_str()),
                percent: c.name("pct").is_some(),
            }
        })
        .collect()
}

/// `valid?`: not a mix of named, numbered and unnumbered sequences.
pub fn is_valid(s: &str) -> bool {
    let mut kinds: Vec<u8> = format_sequences(s)
        .iter()
        .filter(|q| !q.percent)
        .map(|q| if q.name.is_some() { 0 } else if q.max_digit_dollar_num().is_some() { 1 } else { 2 })
        .collect();
    kinds.sort_unstable();
    kinds.dedup();
    kinds.len() <= 1
}

pub fn named_interpolation(s: &str) -> bool {
    format_sequences(s).iter().any(|q| q.name.is_some())
}

/// `max_digit_dollar_num`: Ruby's `[nil, 1].max` raises, so a mix of sequences with and without
/// `N$` is `None` (the cop crashes there and reports nothing).
pub fn max_digit_dollar_num(s: &str) -> Option<Option<u64>> {
    let nums: Vec<Option<u64>> = format_sequences(s).iter().map(FormatSequence::max_digit_dollar_num).collect();
    if nums.iter().all(Option::is_none) {
        return Some(None);
    }
    if nums.iter().any(Option::is_none) {
        return None;
    }
    Some(nums.into_iter().flatten().max())
}

/// The named parts of one format sequence (`FormatSequence#flags`, `width`, `precision`, `name`,
/// `type`), for a `source` that `format_sequences` returned.
pub struct SequenceParts<'a> {
    pub flags: String,
    pub width: Option<&'a str>,
    pub precision: Option<&'a str>,
    pub name: Option<&'a str>,
    pub ty: Option<&'a str>,
}

static PARTS: LazyLock<Regex> = LazyLock::new(|| {
    let digit_dollar = r"[0-9]+\$";
    let flag = format!(r"(?:[ #0+\-]|{digit_dollar})");
    let number = format!(r"(?:[0-9]+|\*(?:{digit_dollar})?|\#\{{.*?\}})");
    let w = |i: u8| format!(r"(?P<w{i}>{number})");
    let p = |i: u8| format!(r"\.(?P<p{i}>{number}?)");
    let typed = format!(
        r"(?:{w1}?(?:{p1})?(?:<(?P<n1>[A-Za-z0-9_]+)>)?|{w2}?<(?P<n2>[A-Za-z0-9_]+)>(?:{p2})?|<(?P<n3>[A-Za-z0-9_]+)>(?P<more>{flag}*){w3}?(?:{p3})?)(?P<ty>[bBdiouxXeEfgGaAcps])",
        w1 = w(1),
        p1 = p(1),
        w2 = w(2),
        p2 = p(2),
        w3 = w(3),
        p3 = p(3),
    );
    let template = format!(r"{w4}?(?:{p4})?\{{(?P<tn>[A-Za-z0-9_]+)\}}", w4 = w(4), p4 = p(4));
    Regex::new(&format!(r"\A%(?P<flags>{flag}*)(?:{typed}|{template})\z")).unwrap()
});

/// Splits a (non-`%%`) format sequence into its parts like Ruby's named captures.
pub fn sequence_parts(source: &str) -> SequenceParts<'_> {
    let empty = SequenceParts { flags: String::new(), width: None, precision: None, name: None, ty: None };
    let Some(c) = PARTS.captures(source) else { return empty };
    let first = |names: &[&str]| names.iter().find_map(|g| c.name(g)).map(|m| m.as_str());
    let flags = format!("{}{}", first(&["flags"]).unwrap_or(""), first(&["more"]).unwrap_or(""));
    SequenceParts {
        flags,
        width: first(&["w1", "w2", "w3", "w4"]),
        precision: first(&["p1", "p2", "p3", "p4"]),
        name: first(&["n1", "n2", "n3", "tn"]),
        ty: first(&["ty"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequences() {
        let s = format_sequences("%s %-5d %<a>s %{b} %% %#{x}s %#{c} %1$*2$d");
        let src: Vec<&str> = s.iter().map(|q| q.source).collect();
        assert_eq!(src, ["%s", "%-5d", "%<a>s", "%{b}", "%%", "%#{x}s", "%1$*2$d"]);
        assert!(!is_valid("%s %<a>s"));
        assert!(is_valid("%s %% %d"));
        assert_eq!(max_digit_dollar_num("%1$s %2$s"), Some(Some(2)));
        assert!(max_digit_dollar_num("%1$s %%").is_none());
    }
}
