use crate::prelude::*;

pub const NAME: &str = "Style/FrozenStringLiteralComment";
pub static COP: FrozenStringLiteralComment = FrozenStringLiteralComment;
pub struct FrozenStringLiteralComment;

static SIMPLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\A\s*#\s*frozen[_-]string[_-]literal:\s*([[:alnum:]\-_]+)\s*\z").unwrap());
static EMACS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-\*-(.+)-\*-").unwrap());
static EMACS_FROZEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\Afrozen[_-]string[_-]literal\s*:\s*([[:alnum:]\-_]+)\z").unwrap());

/// MagicComment#frozen_string_literal: Some("true"/"false"/other token, lowercased), None if unspecified.
pub fn frozen_value(line: &str) -> Option<String> {
    match EMACS.captures(line) {
        Some(e) => e[1].split(';').find_map(|t| EMACS_FROZEN.captures(t.trim()).map(|c| c[1].to_lowercase())),
        None => SIMPLE.captures(line).map(|c| c[1].to_lowercase()),
    }
}

impl Cop for FrozenStringLiteralComment {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        if cx.facts.first_code.is_none() && src.comments.is_empty() {
            return;
        }
        let leading = match cx.facts.first_code {
            Some(off) => &src.lines[..(src.line_of(off) - 1).min(src.lines.len())],
            None => &src.lines[..],
        };
        let values: Vec<String> = leading.iter().filter_map(|l| frozen_value(l)).collect();
        let valid = values.iter().any(|v| v == "true" || v == "false");
        // The first comment token anywhere that specifies the setting.
        let comment = src.comments.iter().find(|c| frozen_value(src.slice(c.start, c.end)).is_some());
        match cx.cfg.str("EnforcedStyle").unwrap_or("always") {
            "never" => {
                if valid && let Some(c) = comment {
                    cx.add(c.start, c.end, "Unnecessary frozen string literal comment.");
                }
            }
            "always_true" => {
                if values.is_empty() {
                    cx.add(0, src.text.chars().next().map_or(0, char::len_utf8), "Missing magic comment `# frozen_string_literal: true`.");
                } else if values[0] != "true"
                    && let Some(c) = comment
                {
                    cx.add(c.start, c.end, "Frozen string literal comment must be set to `true`.");
                }
            }
            _ => {
                if !valid {
                    cx.add(0, src.text.chars().next().map_or(0, char::len_utf8), "Missing frozen string literal comment.");
                }
            }
        }
    }
}
