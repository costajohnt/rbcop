use crate::directives::{self, Directive, DirectiveRange, LineInfo};
use crate::helpers::comment_config::{StatementBounds, code_lines};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Style/DirectiveScope";
pub static COP: DirectiveScope = DirectiveScope;
pub struct DirectiveScope;

/// DirectiveComment#raw_cop_names: `(cops || '').split(/,\s*/)`, sorted.
fn raw_sorted(d: &Directive) -> Vec<&str> {
    static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",[ \t\n\x0B\x0C\r]*").unwrap());
    let mut names: Vec<&str> = SPLIT.split(d.cops.as_deref().unwrap_or("")).collect();
    // Ruby's split drops trailing empty fields.
    while names.last() == Some(&"") {
        names.pop();
    }
    names.sort_unstable();
    names
}

fn plain_disabled(d: &Directive) -> bool {
    d.mode == "disable" || d.mode == "todo"
}

/// DirectiveComment#reason: the text after `--` following the directive.
fn reason(text: &str, d: &Directive) -> Option<String> {
    let ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | '\0');
    let tail = text[d.end..].trim_start_matches(ws);
    let r = tail.strip_prefix("--")?.trim_matches(ws);
    (!r.is_empty()).then(|| r.to_string())
}

struct Ctx<'a> {
    src: &'a crate::source::Source<'a>,
    info: &'a LineInfo<'a>,
    ranges: HashMap<String, Vec<DirectiveRange>>,
    /// ProcessedSource#comment_at_line (the last comment starting on a line).
    at_line: HashMap<usize, usize>,
}

impl Ctx<'_> {
    fn text(&self, i: usize) -> &str {
        let c = &self.src.comments[i];
        self.src.slice(c.start, c.end)
    }
    fn line(&self, i: usize) -> usize {
        self.src.line_of(self.src.comments[i].start)
    }
    fn comment_only(&self, line: usize) -> bool {
        self.info.comment_only(line as i64)
    }
    fn scope_after(&self, line: usize) -> Option<(usize, usize)> {
        directives::statement_scope_after(self.info, self.src, line as i64)
    }
    fn wraps_single_statement(&self, line: usize, closing_line: usize) -> bool {
        self.scope_after(line).is_some_and(|(b, e)| b == line + 1 && e + 1 == closing_line)
    }
    fn whole_line(&self, i: usize) -> (usize, usize) {
        let c = &self.src.comments[i];
        range_by_whole_lines(self.src.text, c.start, c.end, true)
    }
}

/// An offense: message, comment index, replacement text for it, comment index whose line goes.
type Found = (String, usize, String, usize);

fn sub_word(text: &str, word: &str, with: &str) -> String {
    let re = Regex::new(&format!(r"\b{}\b", regex::escape(word))).unwrap();
    re.replace(text, regex::NoExpand(with)).into_owned()
}

fn check_pair(cx: &Ctx, i: usize, d: &Directive) -> Option<Found> {
    let line = cx.line(i);
    // single_closing_line: the one finite end of the ranges this directive opened.
    let mut ends: Vec<i64> =
        cx.ranges.values().flatten().filter(|r| r.2 == Some(i)).map(|r| r.1).collect();
    ends.sort_unstable();
    ends.dedup();
    let [end] = ends.as_slice() else { return None };
    if *end == i64::MAX || *end == i64::MIN {
        return None;
    }
    let closing_line = *end as usize;
    let &closing = cx.at_line.get(&closing_line)?;
    if !cx.wraps_single_statement(line, closing_line) {
        return None;
    }
    let enable = directives::parse(cx.text(closing))?;
    if !enable.enabled() || raw_sorted(&enable) != raw_sorted(d) {
        return None;
    }
    let mode = &d.mode;
    let message = format!("Use `{mode}-next` instead of a `{mode}`/`enable` pair around a single statement.");
    Some((message, i, sub_word(cx.text(i), mode, &format!("{mode}-next")), closing))
}

fn check_push_pop(cx: &Ctx, i: usize, d: &Directive) -> Option<Found> {
    let line = cx.line(i);
    let mut depth = 0;
    let mut pop_line = None;
    for j in 0..cx.src.comments.len() {
        let Some(other) = directives::parse(cx.text(j)) else { continue };
        if cx.line(j) <= line {
            continue;
        }
        if other.mode == "push" {
            depth += 1;
        } else if other.mode == "pop" {
            if depth == 0 {
                pop_line = Some(cx.line(j));
                break;
            }
            depth -= 1;
        }
    }
    let pop_line = pop_line?;
    if !cx.comment_only(pop_line) || !cx.wraps_single_statement(line, pop_line) {
        return None;
    }
    let &pop = cx.at_line.get(&pop_line)?;
    let args = d.signed_args();
    let mut ops: Vec<char> = args.iter().map(|a| a.0).collect();
    ops.sort_unstable();
    ops.dedup();
    let names = |op: char| args.iter().filter(|a| a.0 == op).map(|a| a.1).collect::<Vec<_>>().join(", ");
    let (mode, text) = match ops.as_slice() {
        ['-'] => ("disable-next", format!("# rubocop:disable-next {}", names('-'))),
        ['+'] => ("enable-next", format!("# rubocop:enable-next {}", names('+'))),
        _ => ("next", format!("# rubocop:next {}", d.cops.as_deref().unwrap_or(""))),
    };
    let replacement = match reason(cx.text(i), d) {
        Some(r) => format!("{text} -- {r}"),
        None => text,
    };
    let message = format!("Use `{mode}` instead of `push`/`pop` around a single statement.");
    Some((message, i, replacement, pop))
}

fn check_enable_pair(cx: &Ctx, i: usize, d: &Directive) -> Option<Found> {
    let line = cx.line(i);
    let (begin, end) = cx.scope_after(line)?;
    if begin != line + 1 {
        return None;
    }
    // re_disable_below
    let below = end + 1;
    if !cx.comment_only(below) {
        return None;
    }
    let &closing = cx.at_line.get(&below)?;
    let closer = directives::parse(cx.text(closing))?;
    if !plain_disabled(&closer) || raw_sorted(&closer) != raw_sorted(d) {
        return None;
    }
    // closed_open_disables?
    let closes = directives::cop_names(d).iter().all(|cop| {
        cx.ranges.get(cop).is_some_and(|rs| {
            rs.iter().any(|r| r.1 == line as i64) && rs.iter().any(|r| r.2 == Some(closing))
        })
    });
    if !closes {
        return None;
    }
    let message = "Use `enable-next` instead of an `enable`/`disable` pair around a single statement.".to_string();
    Some((message, i, sub_word(cx.text(i), "enable", "enable-next"), closing))
}

impl Cop for DirectiveScope {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        if !src.text.contains("rubocop") {
            return;
        }
        let code = code_lines(src, cx.facts);
        let bounds = StatementBounds::build(src, node);
        let statement_bounds = |line: usize| bounds.at(line);
        let info = LineInfo { code: &code, statement_bounds: &statement_bounds };
        // Registry#disabled_names(config): every cop off in config starts disabled.
        let config_disabled: Vec<&'static str> =
            crate::cops::ALL.iter().map(|r| r.name).filter(|n| !cx.config.cop(n).enabled).collect();
        let ranges = directives::analyze(src, &config_disabled, &info);
        let at_line = (0..src.comments.len()).map(|i| (src.line_of(src.comments[i].start), i)).collect();
        let ctx = Ctx { src, info: &info, ranges, at_line };
        let mut found = Vec::new();
        for i in 0..src.comments.len() {
            let Some(d) = directives::parse(ctx.text(i)) else { continue };
            if !ctx.comment_only(ctx.line(i)) {
                continue;
            }
            let hit = if plain_disabled(&d) {
                check_pair(&ctx, i, &d)
            } else if d.mode == "push" && !d.signed_args().is_empty() {
                check_push_pop(&ctx, i, &d)
            } else if d.mode == "enable" && d.cops.as_deref() != Some("all") {
                check_enable_pair(&ctx, i, &d)
            } else {
                None
            };
            found.extend(hit);
        }
        for (message, i, replacement, removed) in found {
            let c = &src.comments[i];
            let (s, e) = (c.start, c.end);
            let (rs, re) = ctx.whole_line(removed);
            cx.add_fix(s, e, message, |corr| {
                corr.replace(s, e, &replacement);
                corr.remove(rs, re);
            });
        }
    }
}
