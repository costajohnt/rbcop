use crate::directives::{self, Directive};
use crate::helpers::cop_registry::{REGISTRY, is_department};
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Lint/RedundantCopEnableDirective";
pub static COP: RedundantCopEnableDirective = RedundantCopEnableDirective;
pub struct RedundantCopEnableDirective;

const MSG_ORPHAN_POP: &str = "Unnecessary `rubocop:pop` without a matching `rubocop:push`.";

impl Cop for RedundantCopEnableDirective {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        // processed_source.blank?: no AST (nothing but comments and blank lines).
        if cx.facts.first_code.is_none() {
            return;
        }
        let raw = cx.src.raw_text;
        if raw.contains("enable") {
            check_extra_enables(cx);
        }
        if raw.contains("pop") {
            check_orphan_pops(cx);
        }
    }
}

fn text<'a>(src: &'a crate::source::Source, i: usize) -> &'a str {
    src.slice(src.comments[i].start, src.comments[i].end)
}

/// CommentConfig#extra_enabled_comments: comment index -> names enabled without a disable.
fn extra_enabled_comments(cx: &Cx) -> Vec<(usize, Vec<String>)> {
    let src = cx.src;
    let mut extras: Vec<(usize, Vec<String>)> = Vec::new();
    if !src.text.contains("rubocop") {
        return extras;
    }
    // Counts start at one for registry.disabled_names(config).
    let mut names: HashMap<String, i64> = HashMap::new();
    for c in &cx.facts.config_disabled {
        *names.entry(c.to_string()).or_default() += 1;
    }
    let code = &cx.facts.code_lines;
    for i in 0..src.comments.len() {
        let Some(d) = directives::parse(text(src, i)) else { continue };
        let line = src.line_of(src.comments[i].start);
        if code.get(line).copied().unwrap_or(false) {
            continue;
        }
        if matches!(d.mode.as_str(), "push" | "pop" | "disable-next" | "todo-next" | "next" | "enable-next") {
            continue;
        }
        let mut push = |name: String| match extras.iter_mut().find(|(c, _)| *c == i) {
            Some((_, v)) => v.push(name),
            None => extras.push((i, vec![name])),
        };
        if !d.disabled() && d.cops.as_deref() == Some("all") {
            // handle_enable_all
            let mut enabled = 0;
            for count in names.values_mut() {
                if *count > 0 {
                    *count -= 1;
                    enabled += 1;
                }
            }
            if enabled == 0 {
                push("all".into());
            }
        } else {
            // handle_switch over DirectiveComment#cop_names (not qualified)
            for name in cop_names(&d) {
                let count = names.entry(name.clone()).or_default();
                if d.disabled() {
                    *count += 1;
                } else if *count > 0 {
                    *count -= 1;
                } else {
                    push(name);
                }
            }
        }
    }
    extras
}

/// DirectiveComment#cop_names: `all` is every registered cop except Lint/Syntax and this
/// department's redundancy cop; departments expand; names stay as written.
fn cop_names(d: &Directive) -> Vec<String> {
    if d.cops.as_deref() == Some("all") {
        return REGISTRY.iter().copied().filter(lint_excluded).map(String::from).collect();
    }
    parsed_cop_names(d)
}

fn lint_excluded(c: &&str) -> bool {
    *c != "Lint/RedundantCopDisableDirective" && *c != "Lint/Syntax"
}

/// DirectiveComment#parsed_cop_names.
fn parsed_cop_names(d: &Directive) -> Vec<String> {
    let mut out = Vec::new();
    for raw in d.raw_names() {
        if is_department(raw) {
            let prefix = format!("{raw}/");
            out.extend(
                REGISTRY.iter().copied().filter(|c| c.starts_with(&prefix)).filter(|c| raw != "Lint" || lint_excluded(c)).map(String::from),
            );
        } else {
            out.push(raw.to_string());
        }
    }
    out.retain(|c| c != "Lint/Syntax");
    out
}

/// `comment.text.index(/#{Regexp.escape(name)}(?!\w)/)` (byte offset).
fn index_of(haystack: &str, needle: &str) -> Option<usize> {
    haystack.match_indices(needle).map(|(i, _)| i).find(|&i| {
        !haystack.as_bytes().get(i + needle.len()).is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

fn check_extra_enables(cx: &mut Cx) {
    let src = cx.src;
    for (i, names) in extra_enabled_comments(cx) {
        let c = &src.comments[i];
        let comment = text(src, i);
        let d = directives::parse(comment).unwrap();
        let raw = d.raw_names();
        // DirectiveComment#match?(cop_names)
        let mut parsed: Vec<String> = parsed_cop_names(&d);
        parsed.sort();
        parsed.dedup();
        let mut wanted = names.clone();
        wanted.sort();
        wanted.dedup();
        let whole = parsed == wanted;
        for name in &names {
            let department = raw.iter().any(|n| is_department(n) && name.starts_with(n)) && !raw.contains(&name.as_str());
            let name = if department { name.split('/').next().unwrap_or("").to_string() } else { name.clone() };
            // range_of_offense; RuboCop raises when the name is not in the comment.
            let Some(at) = index_of(comment, &name) else {
                crate::runner::cop_error(NAME, src, c.start);
                return;
            };
            let (s, e) = (c.start + at, c.start + at + name.len());
            let msg = format!("Unnecessary enabling of {}.", if name == "all" { "all cops" } else { &name });
            let removal = if whole {
                let end = with_reason_end(comment, &d) + c.start;
                range_with_surrounding_space(
                    src.text,
                    c.start + d.start,
                    end,
                    Surrounding { side: Side::Right, ..Default::default() },
                )
            } else {
                range_with_comma(comment, c.start, c.end, at, name.len())
            };
            cx.add_fix(s, e, msg, |corr| corr.remove(removal.0, removal.1));
        }
    }
}

/// DirectiveComment#range_with_reason end, relative to the comment.
fn with_reason_end(comment: &str, d: &Directive) -> usize {
    let rest = &comment[d.end..];
    if rest.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r', '\0']).starts_with("--") {
        comment.len()
    } else {
        d.end
    }
}

/// range_with_comma (positions relative to the comment text).
fn range_with_comma(comment: &str, start: usize, end_of_comment: usize, at: usize, len: usize) -> (usize, usize) {
    let b = comment.as_bytes();
    // SurroundingSpace's `SINGLE_SPACE_REGEXP = /[ \t]/`, not Ruby's `\s`.
    let blank = |c: Option<&u8>| matches!(c, Some(b' ' | b'\t'));
    // SurroundingSpace#reposition: src[-1] is the last character.
    let mut begin = at;
    loop {
        let c = if begin == 0 { b.last() } else { b.get(begin - 1) };
        if !blank(c) {
            break;
        }
        if begin == 0 {
            break;
        }
        begin -= 1;
    }
    let mut end = at + len;
    while blank(b.get(end)) {
        end += 1;
    }
    let at_index = |i: isize| if i < 0 { b.last().copied() } else { b.get(i as usize).copied() };
    if at_index(begin as isize - 1) == Some(b',') {
        (start + begin - 1, start + end)
    } else if b.get(end) == Some(&b',') {
        let begin = if b.get(end + 1) != Some(&b' ') { begin + 1 } else { begin };
        (start + begin, start + end + 1)
    } else {
        (start, end_of_comment)
    }
}

fn check_orphan_pops(cx: &mut Cx) {
    let src = cx.src;
    let mut depth = 0;
    let mut found = Vec::new();
    for (i, c) in src.comments.iter().enumerate() {
        let Some(d) = directives::parse(text(src, i)) else { continue };
        if d.mode == "push" {
            depth += 1;
        } else if d.mode == "pop" {
            if depth == 0 {
                found.push((c.start + d.start, c.start + d.end));
            } else {
                depth -= 1;
            }
        }
    }
    for (s, e) in found {
        let r = range_with_surrounding_space(src.text, s, e, Surrounding { side: Side::Right, ..Default::default() });
        cx.add_fix(s, e, MSG_ORPHAN_POP, |corr| corr.remove(r.0, r.1));
    }
}
