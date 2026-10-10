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
    let ws = [' ', '\t', '\n', '\x0b', '\x0c', '\r', '\0'];
    text[d.end..].trim_start_matches(ws).strip_prefix("--").is_some_and(|r| !r.trim_matches(ws).is_empty())
}

impl Cop for DisableCopsWithinSourceCodeDirective {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let allowed_directives = cx.cfg.strings("AllowedDirectives");
        let allow_with_reason = cx.cfg.bool("AllowWithReason").unwrap_or(false);
        let allowed = cx.cfg.strings("AllowedCops");
        let disallowed_cfg = cx.cfg.strings("DisallowedCops");
        for c in &src.comments {
            let text = src.slice(c.start, c.end);
            let Some(d) = directives::parse(text) else { continue };
            // ignored?
            if allowed_directives.contains(&d.mode) || (allow_with_reason && (has_reason(text, &d) || d.enabled())) {
                continue;
            }
            let cops: Vec<&str> =
                d.cops.as_deref().map(|c| c.split(',').map(helpers::ruby_string::strip).collect()).unwrap_or_default();
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
            let replacement = if cops.len() != disallowed.len() {
                let alternatives: Vec<String> = disallowed.iter().map(|c| regex::escape(c)).collect();
                let first = Regex::new(&format!(r"(?:{}),?[ \t\n\x0B\x0C\r]*", alternatives.join("|"))).unwrap();
                let once = first.replace(text, "").into_owned();
                static TRAILING: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"(?m),[ \t\n\x0B\x0C\r]*$").unwrap());
                TRAILING.replace(&once, "").into_owned()
            } else {
                String::new()
            };
            let (s, e) = (c.start, c.end);
            cx.add_fix(s, e, msg, |corr| corr.replace(s, e, &replacement));
        }
    }
}
