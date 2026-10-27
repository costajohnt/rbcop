//! Lint/RedundantCopDisableDirective is not a node or file cop: RuboCop's Runner runs it after all
//! other cops with their offenses (`offenses_to_check`), see `runner::analyze` and
//! `autocorrect::correct`. `check` is that investigation.

use crate::directives::{self, Directive, DirectiveRange};
use crate::helpers::cop_registry::{REGISTRY, is_department};
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use crate::runner::Span;

pub const NAME: &str = "Lint/RedundantCopDisableDirective";
pub static COP: RedundantCopDisableDirective = RedundantCopDisableDirective;
pub struct RedundantCopDisableDirective;

impl Cop for RedundantCopDisableDirective {
    fn correctable(&self) -> bool {
        true
    }
}

const MARKER: &str = "DEPARTMENT";

struct Ctx<'a> {
    src: &'a crate::source::Source<'a>,
    code: &'a [bool],
    checked: &'a [Span],
    own: Vec<(i64, i64)>,
}

impl Ctx<'_> {
    fn text(&self, i: usize) -> &str {
        let c = &self.src.comments[i];
        self.src.slice(c.start, c.end)
    }
    fn directive(&self, i: usize) -> Directive {
        directives::parse(self.text(i)).expect("directive comment")
    }
    fn comment_only(&self, line: usize) -> bool {
        !self.code.get(line).copied().unwrap_or(false)
    }
    /// ignore_offense?: config-disabled ranges and ranges inside a disable of this cop.
    fn ignore(&self, r: &DirectiveRange) -> bool {
        r.0 == i64::MIN || self.own.iter().any(|&(a, b)| a <= r.0 && r.0 <= b && a <= r.1 && r.1 <= b)
    }
    /// range_with_offense?: true when none of `offenses` overlaps the range (sic).
    fn free(&self, r: &DirectiveRange, pick: impl Fn(&str) -> bool) -> bool {
        !self.checked.iter().any(|&(cop, first, last)| pick(cop) && first as i64 <= r.1 && last as i64 >= r.0)
    }
    fn redundant_cop(&self, cop: &str, r: &DirectiveRange) -> Option<String> {
        self.free(r, |c| c == cop).then(|| cop.to_string())
    }
    fn redundant_department(&self, cop: &str, r: &DirectiveRange) -> Option<String> {
        let dept = cop.split('/').next().unwrap_or("");
        self.free(r, |c| c.starts_with(dept)).then(|| format!("{MARKER}{dept}"))
    }
}

/// skip_directive?: config-disabled ranges and push/pop/next directives.
fn skip(d: Option<&Directive>) -> bool {
    d.is_none_or(|d| matches!(d.mode.as_str(), "push" | "pop" | "next"))
}

/// department_disabled?
fn department_disabled(cop: &str, d: &Directive) -> bool {
    let raw = d.raw_names();
    raw.iter().any(|n| is_department(n) && cop.starts_with(n)) && !raw.contains(&cop)
}

fn known(cop: &str) -> bool {
    crate::helpers::cop_registry::is_known(cop)
}

fn registered(cop: &str) -> bool {
    REGISTRY.contains(&cop)
}

/// Registry#find_by_cop_name: Badge.parse camel-cases each part (`lint/self_assignment` finds
/// Lint/SelfAssignment).
fn find_by_cop_name(cop: &str) -> Option<String> {
    let camel = |part: &str| -> String {
        if part == "rspec" {
            return "RSpec".into();
        }
        // gsub(/^[a-z]|_[a-z]/) { |m| m[-1].upcase }
        let chars: Vec<char> = part.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if i == 0 && c.is_ascii_lowercase() {
                out.push(c.to_ascii_uppercase());
            } else if c == '_' && chars.get(i + 1).is_some_and(char::is_ascii_lowercase) {
                out.push(chars[i + 1].to_ascii_uppercase());
                i += 1;
            } else {
                out.push(c);
            }
            i += 1;
        }
        out
    };
    let name = cop.split('/').map(camel).collect::<Vec<_>>().join("/");
    registered(&name).then_some(name)
}

/// Registry#enabled? (cop config, ignoring comment opt-ins).
fn enabled(cx: &Cx, cop: &str) -> bool {
    cx.config.cop_enabled(cop)
}

/// pending_cop_not_run?
fn pending_not_run(cx: &Cx, cop: &str) -> bool {
    known(cop)
        && { let e = cx.config.enable_cop(cop); e == crate::config::value::Value::Str("pending".into()) && !cx.config.enabled_pending_cop(cop, &e) }
}

/// disabled_by_preview?: a cop that only the `Preview` defaults disable runs again as soon as
/// preview is turned off, so its directives are still needed.
fn disabled_by_preview(cx: &Cx, cop: &str) -> bool {
    use crate::config::value::Value;
    if !cx.config.preview_enabled() || !known(cop) {
        return false;
    }
    let preview = helpers::method_preference::DEFAULTS.map(cop).and_then(|c| c.map("Preview"));
    preview.is_some_and(|p| p.get("Enabled") == Some(&Value::Bool(false))) && !enabled(cx, cop)
}

fn unknown(cop: &str) -> bool {
    cop != "all" && !cop.starts_with(MARKER) && !known(cop)
}

/// describe(cop) with SIMILAR_COP_NAMES_CACHE.
fn describe(cop: &str) -> String {
    if cop == "all" {
        return "all cops".into();
    }
    if let Some(dept) = cop.strip_prefix(MARKER) {
        return format!("`{dept}` department");
    }
    if known(cop) {
        return format!("`{cop}`");
    }
    describe_unknown(cop)
}

/// SIMILAR_COP_NAMES_CACHE[cop]: a registered cop of the same base name, else a similar name.
fn describe_unknown(cop: &str) -> String {
    let basename = format!("/{}", cop.rsplit('/').next().unwrap_or(""));
    let similar = REGISTRY.iter().copied().find(|n| n.ends_with(&basename)).or_else(|| {
        let names: Vec<&str> = REGISTRY.iter().copied().filter(|n| *n != cop).collect();
        helpers::did_you_mean::correct(cop, &names).first().copied()
    });
    match similar {
        Some(s) => format!("`{cop}` (did you mean `{s}`?)"),
        None => format!("`{cop}` (unknown cop)"),
    }
}

/// SIMILAR_COP_NAMES_CACHE is process-global: its `all_cop_names` is the registry of the first
/// file (in inspection order) that described an unknown name. `message` (this cop's) gets each
/// unknown name described again with the registry as it was then (`view`, see
/// cop_registry::with_view); None when it describes none.
pub fn redescribe(message: &str, view: Option<usize>) -> Option<String> {
    static RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"`([^`]+)` \((?:did you mean `[^`]+`\?|unknown cop)\)").unwrap());
    if !RE.is_match(message) {
        return None;
    }
    let out = crate::helpers::cop_registry::with_view(view, || {
        RE.replace_all(message, |c: &regex::Captures| describe_unknown(&c[1])).into_owned()
    });
    Some(out)
}

fn message(names: &str, enabling: bool) -> String {
    format!("Unnecessary {} of {names}.", if enabling { "enabling" } else { "disabling" })
}

/// String#blank? (NUL counts as whitespace).
fn blank(s: &str) -> bool {
    s.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0'))
}

/// `haystack.source.index(/#{Regexp.escape(needle)}(?!\w)/)` as a byte offset in the comment.
fn whole_token(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.match_indices(needle).map(|(i, _)| i).find(|&i| {
        !haystack.as_bytes().get(i + needle.len()).is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

pub fn check(cx: &mut Cx, checked: &[Span]) {
    let src = cx.src;
    let facts = cx.facts;
    let cc = &facts.comment_config;
    let own = cc.ranges.get(NAME).map_or(vec![(0, 0)], |rs| rs.iter().map(|&(a, b, _)| (a, b)).collect());
    let ctx = Ctx { src, code: &facts.code_lines, checked, own };

    // comment index -> redundant cop names (a Set, insertion ordered)
    let mut redundant: Vec<(usize, Vec<String>)> = Vec::new();
    let mut add = |i: usize, cop: String| match redundant.iter_mut().find(|(c, _)| *c == i) {
        Some((_, cops)) => {
            if !cops.contains(&cop) {
                cops.push(cop);
            }
        }
        None => redundant.push((i, vec![cop])),
    };

    // each_detached_next_directive
    for &i in &cc.detached {
        let d = ctx.directive(i);
        let names: Vec<&str> =
            if d.mode == "next" { d.signed_args().into_iter().map(|(_, n)| n).collect() } else { d.raw_names() };
        for n in names {
            add(i, n.to_string());
        }
    }

    for cop in &cc.order {
        let Some(ranges) = cc.ranges.get(cop) else { continue };
        // Deliberate difference: a cop RuboCop would run but rbcop cannot (a plugin rbcop does not
        // implement, custom cops) may have offenses there, so its directives are never redundant.
        if pending_not_run(cx, cop) || disabled_by_preview(cx, cop) || crate::helpers::cop_registry::is_unrunnable(cop) {
            continue;
        }
        // each_already_disabled
        for w in ranges.windows(2) {
            let (prev, r) = (&w[0], &w[1]);
            if ctx.ignore(r) || prev.1 != r.0 {
                continue;
            }
            let Some(i) = r.2 else { continue };
            let d = ctx.directive(i);
            if skip(Some(&d)) || d.disabled_all() {
                continue;
            }
            let found =
                if department_disabled(cop, &d) { ctx.redundant_department(cop, r) } else { Some(cop.clone()) };
            if let Some(f) = found {
                add(i, f);
            }
        }
        // each_line_range
        for (idx, r) in ranges.iter().enumerate() {
            // expected_final_disable?
            if ctx.ignore(r) || (r.1 == i64::MAX && find_by_cop_name(cop).is_some_and(|c| !enabled(cx, &c))) {
                continue;
            }
            let d = r.2.map(|i| ctx.directive(i));
            if skip(d.as_ref()) {
                continue;
            }
            let (i, d) = (r.2.unwrap(), d.unwrap());
            let found = if d.disabled_all() {
                let next_follows = ranges.get(idx + 1).is_some_and(|n| r.1 == n.0);
                let unknowable = crate::helpers::cop_registry::has_unrunnable();
                (!next_follows && !unknowable && ctx.free(r, |_| true)).then(|| "all".to_string())
            } else if department_disabled(cop, &d) {
                ctx.redundant_department(cop, r)
            } else {
                ctx.redundant_cop(cop, r)
            };
            if let Some(f) = found {
                add(i, f);
            }
        }
    }

    for (i, cops) in redundant {
        let d = ctx.directive(i);
        if d.disabled_all() || d.count() == cops.len() {
            entire_comment(cx, &ctx, i, &d, cops);
        } else {
            if !some_cops(cx, &ctx, i, cops) {
                return;
            }
        }
    }
}

fn entire_comment(cx: &mut Cx, ctx: &Ctx, i: usize, d: &Directive, mut cops: Vec<String>) {
    let src = ctx.src;
    let c = &src.comments[i];
    let (start, end) = (c.start + d.start, c.start + d.end);
    cops.sort();
    let names: Vec<String> = cops.iter().map(|c| describe(c)).collect();
    let msg = message(&names.join(", "), d.mode == "enable-next");
    let line = src.line_of(c.start);
    let misplaced = matches!(d.mode.as_str(), "disable-next" | "todo-next" | "next" | "enable-next")
        && !ctx.comment_only(line);
    if cops.iter().any(|c| unknown(c)) || misplaced {
        return cx.add_with(start, end, msg, false);
    }
    // range_with_reason
    let text = src.text;
    let rest = &text[end..c.end];
    let dr_end = if rest.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r', '\0']).starts_with("--") {
        c.end
    } else {
        end
    };
    // comment_range_with_surrounding_space; previous_line_blank? reads source_line(line - 1), which
    // for line 1 is the buffer's last line (Ruby's negative fetch).
    let buffer_lines: Vec<&str> = text.split('\n').collect();
    let prev = if line >= 2 { buffer_lines[line - 2] } else { buffer_lines.last().copied().unwrap_or("") };
    let (rs, re) = if blank(prev) && ctx.comment_only(line) && start == c.start {
        range_with_surrounding_space(text, start, dr_end, Surrounding { side: Side::Right, ..Default::default() })
    } else {
        let (s, e) =
            range_with_surrounding_space(text, start, dr_end, Surrounding { side: Side::Left, ..Default::default() });
        let right = Surrounding { side: Side::Right, newlines: start == 0, ..Default::default() };
        range_with_surrounding_space(text, s, e, right)
    };
    // leave_free_comment?
    let comment_text = &text[c.start..c.end];
    let free = comment_text.replace(helpers::ruby_string::strip(&text[rs..re]), "");
    let replace = !free.is_empty() && !free.starts_with('#');
    cx.add_fix(start, end, msg, |corr| {
        if replace {
            corr.replace(rs, re, " # ");
        } else {
            corr.remove(rs, re);
        }
    });
}

/// add_offense_for_some_cops; false when RuboCop would raise (ending the investigation).
fn some_cops(cx: &mut Cx, ctx: &Ctx, i: usize, cops: Vec<String>) -> bool {
    let src = ctx.src;
    let text = src.text;
    let c = &src.comments[i];
    let comment_text = &text[c.start..c.end];
    // cop_range; RuboCop raises (losing the rest) when the name is not in the comment.
    let mut cop_ranges: Vec<(String, usize, usize)> = Vec::new();
    for cop in cops {
        let needle = cop.strip_prefix(MARKER).unwrap_or(&cop);
        let badge = needle.rsplit('/').next().unwrap_or(needle);
        let hit = whole_token(comment_text, needle)
            .map(|o| (o, needle.len()))
            .or_else(|| whole_token(comment_text, badge).map(|o| (o, badge.len())));
        let Some((o, len)) = hit else {
            crate::runner::cop_error(NAME, src, c.start);
            return false;
        };
        cop_ranges.push((cop.clone(), c.start + o, c.start + o + len));
    }
    cop_ranges.sort_by_key(|r| r.1);
    let ranges: Vec<(usize, usize)> = cop_ranges.iter().map(|r| (r.1, r.2)).collect();
    for (k, (cop, s, e)) in cop_ranges.iter().enumerate() {
        let msg = message(&describe(cop), false);
        if unknown(cop) {
            cx.add_with(*s, *e, msg, false);
            continue;
        }
        let (rs, re) = range_in_list(src, k, &ranges);
        cx.add_fix(*s, *e, msg, |corr| corr.remove(rs, re));
    }
    true
}

/// directive_range_in_list
fn range_in_list(src: &crate::source::Source, k: usize, ranges: &[(usize, usize)]) -> (usize, usize) {
    let text = src.text;
    let (mut s, mut e) = ranges[k];
    // ends_its_line?(ranges.last): the last name ends where the line's trailing whitespace starts.
    let last_end = ranges[ranges.len() - 1].1;
    let line = text.split('\n').nth(src.line_of(last_end) - 1).unwrap_or("");
    let trailing_ws = line.trim_end_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']).chars().count();
    let ends_line = trailing_ws == src.column(last_end);
    let trailing = ranges[k..].windows(2).all(|w| {
        let between = &text[w[0].1..w[1].0];
        let t = between.trim_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']);
        t == "," && between.matches(',').count() == 1
    });
    let bytes = text.as_bytes();
    if ends_line && trailing {
        (s, e) = range_with_surrounding_space(text, s, e, Surrounding { side: Side::Left, ..Default::default() });
        while s > 0 && bytes[s - 1] == b',' {
            s -= 1;
        }
    }
    while bytes.get(e) == Some(&b',') {
        e += 1;
    }
    range_with_surrounding_space(text, s, e, Surrounding { side: Side::Right, newlines: false, ..Default::default() })
}
