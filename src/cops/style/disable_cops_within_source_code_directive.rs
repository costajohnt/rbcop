use crate::directives;
use crate::prelude::*;

pub const NAME: &str = "Style/DisableCopsWithinSourceCodeDirective";
pub static COP: DisableCopsWithinSourceCodeDirective = DisableCopsWithinSourceCodeDirective;
pub struct DisableCopsWithinSourceCodeDirective;

const MSG: &str = "RuboCop disable/enable directives are not permitted.";
const MSG_MISSING_REASON: &str = "RuboCop disable directives without a `--` justification comment are not permitted.";

/// `names.include?(cop) || names.include?(cop.split('/').first)`
fn listed(names: &[String], cop: &str) -> bool {
    names.iter().any(|n| n == cop || Some(n.as_str()) == cop.split('/').next())
}

/// DirectiveComment#reason is present (non-empty text after `--`).
fn has_reason(text: &str, d: &directives::Directive) -> bool {
    directives::reason(text, d).is_some()
}

/// `directive.signed_args.values.flatten`: names grouped by operation, in first-seen order.
fn signed_cops(d: &directives::Directive) -> Vec<&str> {
    let args = d.signed_args();
    let mut ops: Vec<char> = Vec::new();
    for (op, _) in &args {
        if !ops.contains(op) {
            ops.push(*op);
        }
    }
    ops.iter().flat_map(|o| args.iter().filter(move |(op, _)| op == o).map(|(_, n)| *n)).collect()
}

/// `Regexp.union(cops)`, longest first so the first alternative that matches is the one
/// Ruby's backtracking would settle on before a `(?!\S)`.
fn union(cops: &[&str]) -> String {
    let mut v: Vec<&str> = cops.to_vec();
    v.sort_by_key(|c| std::cmp::Reverse(c.len()));
    v.iter().map(|c| regex::escape(c)).collect::<Vec<_>>().join("|")
}

/// remove_signed_args: `range.source.gsub(/\s+[+-]#{names}(?!\S)/, '')`.
fn remove_signed(range: &str, cops: &[&str]) -> String {
    let re = Regex::new(&format!(r"[ \t\n\x0B\x0C\r]+[+-](?:{})", union(cops))).unwrap();
    let mut out = String::new();
    let mut last = 0;
    for m in re.find_iter(range) {
        if range[m.end()..].chars().next().is_some_and(|ch| !" \t\n\x0B\x0C\r".contains(ch)) {
            continue;
        }
        out.push_str(&range[last..m.start()]);
        last = m.end();
    }
    out.push_str(&range[last..]);
    out
}

impl Cop for DisableCopsWithinSourceCodeDirective {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let allowed_directives = cx.cfg.strings("AllowedDirectives");
        // The obsolete `AllowTrailingComment` wins when set (backward compatibility).
        let allow_with_reason =
            cx.cfg.bool("AllowTrailingComment").or_else(|| cx.cfg.bool("AllowWithReason")).unwrap_or(false);
        let allowed = cx.cfg.strings("AllowedCops");
        let disallowed_cfg = cx.cfg.strings("DisallowedCops");
        // pair_pushes_with_pops: each `pop` closes the latest unclosed `push`.
        let mut pairs: Vec<(usize, (usize, usize))> = Vec::new();
        let mut pushes: Vec<usize> = Vec::new();
        for c in &src.comments {
            match directives::parse(src.slice(c.start, c.end)).map(|d| d.mode) {
                Some(m) if m == "push" => pushes.push(c.start),
                Some(m) if m == "pop" => {
                    if let Some(p) = pushes.pop() {
                        pairs.push((p, (c.start, c.end)));
                    }
                }
                _ => {}
            }
        }
        for c in &src.comments {
            let text = src.slice(c.start, c.end);
            let Some(d) = directives::parse(text) else { continue };
            // ignored?
            if allowed_directives.contains(&d.mode) || (allow_with_reason && (has_reason(text, &d) || d.enabled())) {
                continue;
            }
            let cops: Vec<&str> = if d.signed() {
                signed_cops(&d)
            } else {
                d.cops.as_deref().map(|c| c.split(',').map(helpers::ruby_string::strip).collect()).unwrap_or_default()
            };
            let disallowed: Vec<&str> = if !disallowed_cfg.is_empty() {
                if cops.contains(&"all") {
                    cops.clone()
                } else {
                    let mut uniq: Vec<&str> = Vec::new();
                    for c in &cops {
                        if !uniq.contains(c) {
                            uniq.push(c);
                        }
                    }
                    uniq.into_iter().filter(|c| listed(&disallowed_cfg, c)).collect()
                }
            } else {
                cops.iter().copied().filter(|c| *c == "all" || !listed(&allowed, c)).collect()
            };
            if disallowed.is_empty() {
                continue;
            }
            let msg = if allow_with_reason {
                MSG_MISSING_REASON.to_string()
            } else if !allowed.is_empty() || !disallowed_cfg.is_empty() {
                format!("RuboCop disable/enable directives for `{}` are not permitted.", disallowed.join("`, `"))
            } else {
                MSG.to_string()
            };
            let (s, e) = (c.start, c.end);
            let mut edits: Vec<(usize, usize, String)> = Vec::new();
            if allow_with_reason {
                // Only a human can write the missing justification.
            } else if cops.len() == disallowed.len() {
                // remove_directive: a `push` goes together with its `pop`.
                edits.push((s, e, String::new()));
                if d.mode == "push"
                    && let Some(&(_, (ps, pe))) = pairs.iter().find(|(p, _)| *p == s)
                {
                    edits.push((ps, pe, String::new()));
                }
            } else if d.signed() {
                edits.push((s + d.start, s + d.end, remove_signed(&text[d.start..d.end], &disallowed)));
            } else {
                let names: Vec<String> = disallowed.iter().map(|c| regex::escape(c)).collect();
                let first = Regex::new(&format!(r"(?:{}),?[ \t\n\x0B\x0C\r]*", names.join("|"))).unwrap();
                let once = first.replace(text, "").into_owned();
                static TRAILING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m),[ \t\n\x0B\x0C\r]*$").unwrap());
                edits.push((s, e, TRAILING.replace(&once, "").into_owned()));
            }
            cx.add_fix(s, e, msg, |corr| {
                for (a, b, r) in &edits {
                    corr.replace(*a, *b, r);
                }
            });
        }
    }
}
