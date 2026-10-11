//! `# rubocop:...` directive comments, ported from RuboCop 1.91's DirectiveComment and CommentConfig
//! (with its DisableNext and PushPop parts).

use crate::source::Source;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

pub static DIRECTIVE: LazyLock<Regex> = LazyLock::new(|| {
    let name = r"(?:[A-Za-z]\w+/)*[A-Za-z]\w+";
    let names = format!(r"(?:{name}\s*,\s*)*{name}");
    let signed = format!(r"[+\-]{name}(?:\s+[+\-]{name})*");
    // AVAILABLE_MODES sorted longest first, as RuboCop builds MODES_PATTERN.
    let modes = "disable-next|enable-next|todo-next|disable|enable|todo|push|next|pop";
    // Ruby's \s and \w are ASCII-only.
    let re = format!(r"#\s*rubocop\s*:\s*({modes})\b(?:\s*(all|{names})|\s+({signed}))?");
    Regex::new(&crate::helpers::ruby_string::ruby_regex_classes(&re)).unwrap()
});

const LINT_SYNTAX: &str = "Lint/Syntax";
const LINT_REDUNDANT: &str = "Lint/RedundantCopDisableDirective";
const STYLE_DISABLE_COPS: &str = "Style/DisableCopsWithinSourceCodeDirective";

pub struct Directive {
    pub mode: String,
    /// The cop list (`all`, names) or the signed `+Name -Name` arguments of push/next.
    pub cops: Option<String>,
    /// Byte offsets of the match within the comment text.
    pub start: usize,
    pub end: usize,
}

impl Directive {
    pub fn disabled(&self) -> bool {
        matches!(self.mode.as_str(), "disable" | "todo" | "disable-next" | "todo-next")
    }
    pub fn enabled(&self) -> bool {
        self.mode == "enable" || self.mode == "enable-next"
    }
    pub fn signed(&self) -> bool {
        self.mode == "push" || self.mode == "next"
    }
    /// DirectiveComment#raw_cop_names: `(cops || '').split(/,\s*/)`, untrimmed (Ruby's split drops
    /// trailing empty fields).
    pub fn raw_names(&self) -> Vec<&str> {
        static COMMA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",[ \t\n\x0B\x0C\r]*").unwrap());
        let mut names: Vec<&str> = COMMA.split(self.cops.as_deref().unwrap_or("")).collect();
        while names.last() == Some(&"") {
            names.pop();
        }
        names
    }
    /// DirectiveComment#directive_count.
    pub fn count(&self) -> usize {
        if self.signed() { self.signed_args().len() } else { self.raw_names().len() }
    }
    /// DirectiveComment#disabled_all?
    pub fn disabled_all(&self) -> bool {
        self.disabled() && self.cops.as_deref() == Some("all")
    }
    /// DirectiveComment#signed_args: (op, name) pairs.
    pub fn signed_args(&self) -> Vec<(char, &str)> {
        if !self.signed() {
            return Vec::new();
        }
        self.cops
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .filter_map(|spec| {
                let op = spec.chars().next()?;
                matches!(op, '+' | '-').then(|| (op, &spec[1..]))
            })
            .collect()
    }
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

/// Cop names RuboCop's registry knows (every cop in default.yml, ported or not).
fn known() -> impl Iterator<Item = &'static str> {
    crate::helpers::cop_registry::REGISTRY.iter().copied()
}

fn is_department(name: &str) -> bool {
    let prefix = format!("{name}/");
    known().any(|c| c.starts_with(&prefix))
}

fn department_cops(dept: &str) -> Vec<&'static str> {
    let prefix = format!("{dept}/");
    known()
        .filter(|c| c.starts_with(&prefix))
        .filter(|c| dept != "Lint" || (*c != LINT_SYNTAX && *c != LINT_REDUNDANT))
        .collect()
}

/// Registry.qualified_cop_name: a bare name becomes the unique cop with that name; anything else
/// is returned unchanged.
fn qualify(name: &str) -> String {
    let name = name.trim();
    if name.contains('/') {
        return name.to_string();
    }
    let suffix = format!("/{name}");
    let mut hits = known().filter(|c| c.ends_with(&suffix));
    match (hits.next(), hits.next()) {
        (Some(one), None) => one.to_string(),
        _ => name.to_string(),
    }
}

/// DirectiveComment#cop_names (departments expanded, Lint/Syntax dropped; `all` = every cop
/// except Lint/Syntax and Lint/RedundantCopDisableDirective), then qualified.
pub fn cop_names(d: &Directive) -> Vec<String> {
    if d.cops.as_deref() == Some("all") {
        return known().filter(|c| *c != LINT_SYNTAX && *c != LINT_REDUNDANT).map(String::from).collect();
    }
    let mut out = Vec::new();
    for raw in d.raw_names() {
        if is_department(raw) {
            out.extend(department_cops(raw).into_iter().map(String::from));
        } else if raw != LINT_SYNTAX {
            out.push(qualify(raw));
        }
    }
    out
}

/// PushPop#resolve_push_cops: signed args with departments expanded.
fn resolve_signed(d: &Directive) -> Vec<(char, String)> {
    let mut out = Vec::new();
    for (op, name) in d.signed_args() {
        if is_department(name) {
            out.extend(department_cops(name).into_iter().map(|c| (op, c.to_string())));
        } else {
            out.push((op, qualify(name)));
        }
    }
    out
}

/// CommentConfig#opt_in_cops: raw names after `enable`/`enable-next` (not `all`) and `+` args of
/// push/next. Raw names are compared with qualified cop names, so `enable LineLength` opts nothing in.
pub fn opted_in(src: &Source) -> Vec<&'static str> {
    if !src.text.contains("rubocop") {
        return Vec::new();
    }
    let mut raw: Vec<String> = Vec::new();
    for c in &src.comments {
        let Some(d) = parse(src.slice(c.start, c.end)) else { continue };
        if d.enabled() {
            if d.cops.as_deref() != Some("all") {
                raw.extend(d.raw_names().into_iter().map(String::from));
            }
        } else if d.signed() {
            raw.extend(d.signed_args().into_iter().filter(|(op, _)| *op == '+').map(|(_, n)| n.to_string()));
        }
    }
    let mut out: Vec<&'static str> = known().filter(|k| raw.iter().any(|r| r == k)).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// What CommentConfig needs to know about the file's lines.
pub struct LineInfo<'a> {
    /// `code[line]`: a non-comment token starts on that line (RuboCop's non_comment_token_line_numbers).
    pub code: &'a [bool],
    /// DisableNext#statement_bounds_at for a code line: the line range of the statement starting there.
    pub statement_bounds: &'a dyn Fn(usize) -> (usize, usize),
}

impl LineInfo<'_> {
    pub fn comment_only(&self, line: i64) -> bool {
        line < 1 || !self.code.get(line as usize).copied().unwrap_or(false)
    }
}

/// A disabled line range and the index (in `src.comments`) of the directive that opened it
/// (DirectiveRange#directive; None for cops disabled in config).
pub type DirectiveRange = (i64, i64, Option<usize>);

#[derive(Clone, Default)]
struct Analysis {
    ranges: Vec<DirectiveRange>,
    start: Option<i64>,
    start_directive: Option<usize>,
}

impl Analysis {
    /// CopAnalysis#close.
    fn close(&self, line: i64) -> Vec<DirectiveRange> {
        let mut r = self.ranges.clone();
        if let Some(s) = self.start
            && line >= s
        {
            r.push((s, line, self.start_directive));
        }
        r
    }
}

/// Whether a source uses any statement-scoped directive (so the runner only computes statement
/// bounds when needed).
pub fn needs_statement_bounds(src: &Source) -> bool {
    src.text.contains("rubocop")
        && src.comments.iter().any(|c| parse(src.slice(c.start, c.end)).is_some_and(|d| d.mode.ends_with("next")))
}

/// CommentConfig#cop_disabled_line_ranges: cop name to inclusive line ranges where it is disabled.
/// `config_disabled` are cops off in config (they start disabled from before line 1).
pub fn disabled_ranges(cc: &CommentConfig) -> HashMap<String, Vec<(i64, i64)>> {
    cc.ranges.iter().map(|(cop, ranges)| (cop.clone(), ranges.iter().map(|&(a, b, _)| (a, b)).collect())).collect()
}

/// CommentConfig#prevent_directive_disabling?: `config.dig(cop, 'Enabled') == true`.
pub fn prevent_style(config: &crate::config::Resolved) -> bool {
    matches!(config.for_cop(STYLE_DISABLE_COPS).get("Enabled"), Some(crate::config::value::Value::Bool(true)))
}

/// The result of CommentConfig#analyze.
#[derive(Default)]
pub struct CommentConfig {
    /// cop_disabled_line_ranges, each range keeping its opening directive.
    pub ranges: HashMap<String, Vec<DirectiveRange>>,
    /// The keys of `ranges` in Ruby hash (first access) order.
    pub order: Vec<String>,
    /// DisableNext#detached_next_directives: comment indexes of next-statement directives that
    /// attach to no statement.
    pub detached: Vec<usize>,
}

/// CommentConfig#analyze. `config_disabled` are cops off in config (they start disabled from
/// before line 1); `prevent_style` is `prevent_directive_disabling?` (Style/DisableCopsWithinSourceCodeDirective
/// is `Enabled: true` in config, so no directive disables it).
pub fn analyze(src: &Source, config_disabled: &[&'static str], lines: &LineInfo, prevent_style: bool) -> CommentConfig {
    if !src.text.contains("rubocop") {
        return CommentConfig::default();
    }
    let mut analyses: HashMap<String, Analysis> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut detached = Vec::new();
    for cop in config_disabled {
        order.push(cop.to_string());
        analyses.insert(cop.to_string(), Analysis { start: Some(i64::MIN), ..Analysis::default() });
    }
    // `analyses[cop]` on Ruby's default-block hash: creates the entry, remembering the order.
    fn entry<'a>(
        analyses: &'a mut HashMap<String, Analysis>,
        order: &mut Vec<String>,
        cop: String,
    ) -> &'a mut Analysis {
        if !analyses.contains_key(&cop) {
            order.push(cop.clone());
        }
        analyses.entry(cop).or_default()
    }
    let mut stack: Vec<HashMap<String, Analysis>> = Vec::new();
    for (index, c) in src.comments.iter().enumerate() {
        let Some(d) = parse(src.slice(c.start, c.end)) else { continue };
        let line = src.line_of(c.start) as i64;
        match d.mode.as_str() {
            "push" => {
                stack.push(analyses.clone());
                for (op, cop) in resolve_signed(&d) {
                    let a = entry(&mut analyses, &mut order, cop);
                    if op == '-' && a.start.is_none() {
                        a.start = Some(line);
                        a.start_directive = Some(index);
                    } else if op == '+' && a.start.is_some() {
                        *a = Analysis { ranges: a.close(line), ..Analysis::default() };
                    }
                }
            }
            "pop" => {
                if let Some(restore) = stack.pop() {
                    let keys: Vec<String> = restore.keys().chain(analyses.keys()).cloned().collect();
                    for cop in keys {
                        let current = analyses.get(&cop).cloned().unwrap_or_default();
                        let ranges = current.close(line - 1);
                        let restored = restore.get(&cop).filter(|r| r.start.is_some());
                        let start = restored.map(|_| line);
                        let start_directive = restored.and_then(|r| r.start_directive);
                        analyses.insert(cop, Analysis { ranges, start, start_directive });
                    }
                }
            }
            "disable-next" | "todo-next" | "next" | "enable-next" => {
                let Some((first, last)) = statement_scope_after(lines, src, line) else {
                    detached.push(index);
                    continue;
                };
                let (first, last) = (first as i64, last as i64);
                let ops: Vec<(char, String)> = match d.mode.as_str() {
                    "next" => resolve_signed(&d),
                    "enable-next" => cop_names(&d).into_iter().map(|c| ('+', c)).collect(),
                    _ => cop_names(&d).into_iter().map(|c| ('-', c)).collect(),
                };
                for (op, cop) in ops {
                    let a = entry(&mut analyses, &mut order, cop);
                    if op == '-' {
                        a.ranges.push((first, last, Some(index)));
                    } else if a.start.is_some() {
                        // suspend_disable: close before the statement, reopen after it.
                        *a = Analysis { ranges: a.close(first - 1), start: Some(last + 1), ..a.clone() };
                    }
                }
            }
            _ => {
                let single_line = d.start != 0;
                for cop in cop_names(&d) {
                    let a = entry(&mut analyses, &mut order, cop);
                    if !lines.comment_only(line) || single_line {
                        if d.disabled() {
                            a.ranges.push((line, line, Some(index)));
                        }
                    } else if d.disabled() {
                        *a = Analysis { ranges: a.close(line), start: Some(line), start_directive: Some(index) };
                    } else {
                        *a = Analysis { ranges: a.close(line), ..Analysis::default() };
                    }
                }
            }
        }
    }
    let keep = |cop: &String| !(prevent_style && cop == STYLE_DISABLE_COPS);
    order.retain(keep);
    let ranges = analyses.into_iter().filter(|(cop, _)| keep(cop)).map(|(cop, a)| (cop, a.close(i64::MAX))).collect();
    CommentConfig { ranges, order, detached }
}

/// DisableNext#statement_scope_after: the statement right below a comment-only directive line.
pub fn statement_scope_after(lines: &LineInfo, src: &Source, directive_line: i64) -> Option<(usize, usize)> {
    if !lines.comment_only(directive_line) {
        return None;
    }
    // attached_code_line: the next line with code, unless a blank line comes first.
    for l in (directive_line as usize + 1)..=src.lines.len() {
        if !lines.comment_only(l as i64) {
            return Some((lines.statement_bounds)(l));
        }
        if src.lines[l - 1].trim().is_empty() {
            return None;
        }
    }
    None
}

static IGNORE_COMMENTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `--ignore-disable-comments`: Cop::Base#enabled_line(s)? is true everywhere for the run.
pub fn ignore_disable_comments() {
    IGNORE_COMMENTS.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Cop::Base#enabled_lines? negated: whether a directive suppresses an offense on these lines
/// (never under `--ignore-disable-comments`). Code that reads the comment config itself
/// (`comment_config.cop_enabled_at_line?`) calls `is_disabled`.
pub fn suppressed(ranges: &HashMap<String, Vec<(i64, i64)>>, cop: &str, first: usize, last: usize) -> bool {
    !IGNORE_COMMENTS.load(std::sync::atomic::Ordering::Relaxed) && is_disabled(ranges, cop, first, last)
}

/// An offense on these lines falls in a range where `cop` is disabled by a comment.
pub fn is_disabled(ranges: &HashMap<String, Vec<(i64, i64)>>, cop: &str, first: usize, last: usize) -> bool {
    ranges.get(cop).is_some_and(|rs| rs.iter().any(|&(a, b)| b >= first as i64 && a <= last as i64))
}
