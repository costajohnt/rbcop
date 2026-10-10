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
}

impl Declaration {
    /// From a call whose first argument is a plain string (`(send ... (str _) ...)`).
    pub fn of(call: &ruby_prism::CallNode) -> Option<Self> {
        let first = call_args(call).into_iter().next()?;
        let name = String::from_utf8_lossy(first.as_string_node()?.unescaped()).into_owned();
        let (start, end) = send_range(call);
        Some(Declaration { start, end, name, method: method_name(call).to_string() })
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
fn first_line(cx: &Cx, start: usize) -> usize {
    let src = cx.src;
    if cx.cfg.bool("TreatCommentsAsGroupSeparators") == Some(true) {
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
/// `msg` gets (current name, previous name).
pub fn check_pair(cx: &mut Cx, previous: &Declaration, current: &Declaration, msg: fn(&str, &str) -> String) {
    if cx.src.line_of(previous.end) + 1 != first_line(cx, current.start) {
        return;
    }
    if canonical(cx, &current.name) < canonical(cx, &previous.name) {
        cx.add(current.start, current.end, msg(&current.name, &previous.name));
    }
}
