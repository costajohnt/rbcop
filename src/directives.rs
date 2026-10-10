//! `# rubocop:disable / enable / todo / push / pop` comments (DirectiveComment + CommentConfig).

use crate::source::Source;
use regex::Regex;
use std::sync::LazyLock;

pub static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
    let name = r"(?:[A-Za-z]\w+/)*[A-Za-z]\w+";
    let names = format!(r"(?:{name}\s*,\s*)*{name}");
    let push_pop = format!(r"[+\-]{name}(?:\s+[+\-]{name})*");
    Regex::new(&format!(r"#\s*rubocop\s*:\s*(disable|enable|todo|push|pop)\b(?:\s*(all|{names})|\s+({push_pop}))?"))
        .unwrap()
});

pub struct Directive {
    pub mode: String,
    /// Raw cop/department names, `all`, or push/pop `+Name -Name` arguments.
    pub cops: Option<String>,
    /// Offset of the match inside the comment text.
    pub start: usize,
    pub end: usize,
}

/// Parse a comment's directive, unless it is commented out (`## rubocop:...`).
pub fn parse(comment: &str) -> Option<Directive> {
    let m = DIRECTIVE.captures(comment)?;
    let whole = m.get(0).unwrap();
    let pre = &comment[..whole.start()];
    if pre.starts_with('#') && pre[1..].trim().is_empty() {
        return None;
    }
    let cops = m.get(2).or(m.get(3)).map(|c| c.as_str().to_string());
    Some(Directive { mode: m[1].to_string(), cops, start: whole.start(), end: whole.end() })
}

/// All cop names in the registry plus those RuboCop knows (for departments and `all`).
fn known() -> impl Iterator<Item = &'static str> {
    crate::cops::ALL.iter().map(|r| r.name).chain(std::iter::once("Lint/Syntax"))
}

/// `all`, departments and unqualified names to qualified cop names.
pub fn expand(names: &str) -> Vec<&'static str> {
    if names == "all" {
        return known().filter(|n| *n != "Lint/Syntax").collect();
    }
    let mut out = Vec::new();
    for n in names.split(',').map(str::trim) {
        let mut hits: Vec<&'static str> =
            known().filter(|c| *c == n || c.strip_prefix(n).is_some_and(|r| r.starts_with('/'))).collect();
        if hits.is_empty() {
            let suffix = format!("/{n}");
            let mut bare = known().filter(|c| c.ends_with(&suffix));
            if let (Some(one), None) = (bare.next(), bare.next()) {
                hits.push(one);
            }
        }
        out.extend(hits.into_iter().filter(|c| *c != "Lint/Syntax"));
    }
    out
}

/// Cops a `# rubocop:enable X` comment opts in for this file (CommentConfig#cop_opted_in?).
pub fn opted_in(src: &Source) -> Vec<&'static str> {
    if !src.text.contains("rubocop") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for c in &src.comments {
        if let Some(d) = parse(src.slice(c.start, c.end))
            && d.mode == "enable"
            && d.cops.as_deref().is_some_and(|c| c != "all")
        {
            out.extend(expand(d.cops.as_deref().unwrap()));
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// (cop, first line, last line) spans where cops are switched off.
/// `config_disabled` lists cops switched off in config; they start disabled (CommentConfig injects a
/// `# rubocop:disable` before line 1), so a `# rubocop:enable` only turns them on from that line.
pub fn disabled_spans(src: &Source, config_disabled: &[&'static str]) -> Vec<(&'static str, usize, usize)> {
    let mut spans = Vec::new();
    if !src.text.contains("rubocop") {
        return spans;
    }
    let mut open: Vec<(&'static str, usize)> = config_disabled.iter().map(|&c| (c, 0)).collect();
    let mut saved: Vec<Vec<(&'static str, usize)>> = Vec::new();
    for c in &src.comments {
        let comment = src.slice(c.start, c.end);
        let Some(d) = parse(comment) else { continue };
        let line = src.line_of(c.start);
        match d.mode.as_str() {
            "push" => {
                saved.push(open.clone());
                for arg in d.cops.as_deref().unwrap_or("").split_whitespace() {
                    let (op, name) = arg.split_at(1);
                    for cop in expand(name) {
                        let pos = open.iter().position(|&(c, _)| c == cop);
                        match (op, pos) {
                            ("-", None) => open.push((cop, line)),
                            ("+", Some(i)) => {
                                let (_, start) = open.remove(i);
                                spans.push((cop, start, line));
                            }
                            _ => {}
                        }
                    }
                }
                continue;
            }
            "pop" => {
                let restore = saved.pop().unwrap_or_default();
                for &(cop, start) in &open {
                    spans.push((cop, start, line.saturating_sub(1)));
                }
                open = restore.iter().map(|&(cop, _)| (cop, line)).collect();
                continue;
            }
            _ => {}
        }
        let Some(names) = d.cops.as_deref() else { continue };
        let disable = d.mode == "disable" || d.mode == "todo";
        let comment_only = src.begins_its_line(c.start);
        let single_line = d.start != 0;
        for cop in expand(names) {
            if !comment_only || single_line {
                if disable {
                    spans.push((cop, line, line));
                }
                continue;
            }
            if let Some(i) = open.iter().position(|&(c, _)| c == cop) {
                let (_, start) = open.remove(i);
                spans.push((cop, start, line));
            }
            if disable {
                open.push((cop, line));
            }
        }
    }
    spans.extend(open.into_iter().map(|(cop, start)| (cop, start, usize::MAX)));
    spans
}

pub fn is_disabled(spans: &[(&'static str, usize, usize)], cop: &str, line: usize) -> bool {
    spans.iter().any(|&(c, a, b)| c == cop && a <= line && line <= b)
}
