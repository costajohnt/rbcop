//! RuboCop's OrderedGemNode mixin (Bundler/OrderedGems, Gemspec/OrderedDependencies).

use crate::cop::Cx;
use crate::prelude::*;

/// A `gem 'x'` / `spec.add_dependency 'x'` declaration: parser `send` range and gem name.
pub struct Declaration {
    pub start: usize,
    pub end: usize,
    pub name: String,
    /// Method name (OrderedDependencies only compares declarations of the same method).
    pub method: String,
    /// No enclosing node starts at the send (`gem 'a' if x` wraps it in a modifier): parser's
    /// Comment::Associator hands a leading comment to the outermost node starting there.
    pub outermost: bool,
}

impl Declaration {
    /// From a call whose first argument is a plain string (`(send ... (str _) ...)`); `ancestors`
    /// are the call's Prism ancestors (`cx.ancestors`).
    pub fn of(call: &ruby_prism::CallNode, ancestors: &[Node]) -> Option<Self> {
        let first = call_args(call).into_iter().next()?;
        let name = String::from_utf8_lossy(first.as_string_node()?.unescaped()).into_owned();
        let (start, end) = send_range(call);
        // A statement list starting here is the statement itself (or a parser `begin`, which never
        // takes leading comments).
        let outermost = !ancestors.iter().any(|a| {
            a.location().start_offset() == start && !matches!(kind::of(a), kind::PROGRAM_NODE | kind::STATEMENTS_NODE)
        });
        Some(Declaration { start, end, name, method: method_name(call).to_string(), outermost })
    }
}

fn canonical(cx: &Cx, name: &str) -> String {
    let name = if cx.cfg.bool("ConsiderPunctuation") == Some(true) { name.to_string() } else { name.replace(['-', '_'], "") };
    name.to_lowercase()
}

static MAGIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#\s*(-\*-)?\s*(frozen_string_literal|warn_indent|warn_past_scope):").unwrap());
static ENCODING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s#](en)?coding\s*[:=]\s*[\w-]+").unwrap());

/// Line of `get_source_range(node, comments_as_separators)`: with TreatCommentsAsGroupSeparators
/// off, the first comment parser's Comment::Associator attaches to the node.
fn first_line(cx: &Cx, d: &Declaration) -> usize {
    let src = cx.src;
    let start = d.start;
    if !d.outermost || cx.cfg.bool("TreatCommentsAsGroupSeparators") == Some(true) {
        return src.line_of(start);
    }
    // Associator skips a leading shebang, magic comment and encoding comment (in that order).
    let mut skip = 0;
    let checks: [fn(&str) -> bool; 3] =
        [|t| t.starts_with("#!"), |t| MAGIC.is_match(t), |t| ENCODING.is_match(t)];
    for check in checks {
        if src.comments.get(skip).is_some_and(|c| check(src.slice(c.start, c.end))) {
            skip += 1;
        }
    }
    // End of the code before the node: skip back over whitespace and comments.
    let mut prev = start;
    loop {
        let trimmed = src.text[..prev].trim_end().len();
        match src.comments.iter().find(|c| c.start < trimmed && c.end >= trimmed && c.end <= prev) {
            Some(c) => prev = c.start,
            _ => {
                prev = trimmed;
                break;
            }
        }
    }
    // ponytail: a comment on the previous code's last line is taken to decorate that code (true
    // for calls; after a bare `begin`/`else` keyword parser would hand it to the next statement).
    src.comments[skip..]
        .iter()
        .find(|c| c.start >= prev && c.end <= start && (prev == 0 || src.line_of(c.start) > src.line_of(prev)))
        .map_or(src.line_of(start), |c| src.line_of(c.start))
}

/// OrderedGemNode: report `current` if it directly follows `previous` and sorts before it.
/// `msg` gets (current name, previous name). The fix moves `current` above `swap_with` (the cop's
/// `previous_declaration(current)`, normally `previous`).
pub fn check_pair(
    cx: &mut Cx,
    previous: &Declaration,
    current: &Declaration,
    swap_with: &Declaration,
    msg: fn(&str, &str) -> String,
) {
    if cx.src.line_of(previous.end) + 1 != first_line(cx, current) {
        return;
    }
    if canonical(cx, &current.name) >= canonical(cx, &previous.name) {
        return;
    }
    // OrderedGemCorrector.correct
    let cur = declaration_with_comment(cx, current);
    let prev = declaration_with_comment(cx, swap_with);
    let text = cx.src.text;
    cx.add_fix(current.start, current.end, msg(&current.name, &previous.name), |c| {
        if text[cur.0..cur.1].ends_with('\n') {
            c.swap(cur, prev);
        } else {
            c.insert_before(prev.0, prev.1, &format!("{}\n", &text[cur.0..cur.1]));
            c.remove(cur.0, cur.1);
        }
    });
}

/// OrderedGemCorrector#declaration_with_comment: whole lines from the first associated comment
/// (or the node) through the node's last line, final newline included.
fn declaration_with_comment(cx: &Cx, d: &Declaration) -> (usize, usize) {
    let src = cx.src;
    let begin = src.line_start(first_line(cx, d));
    let end_line_start = src.line_start(src.line_of(d.end));
    let end = src.text[end_line_start..].find('\n').map_or(src.text.len(), |i| end_line_start + i + 1);
    (begin, end)
}
