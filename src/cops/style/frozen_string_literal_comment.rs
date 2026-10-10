use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Style/FrozenStringLiteralComment";
pub static COP: FrozenStringLiteralComment = FrozenStringLiteralComment;
pub struct FrozenStringLiteralComment;

static SIMPLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\A\s*#\s*frozen[_-]string[_-]literal:\s*([[:alnum:]\-_]+)\s*\z").unwrap());
static EMACS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-\*-(.+)-\*-").unwrap());
static EMACS_FROZEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\Afrozen[_-]string[_-]literal\s*:\s*([[:alnum:]\-_]+)\z").unwrap());
/// Style/Encoding::ENCODING_PATTERN
static ENCODING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#.*coding\s?[:=]\s?(?:UTF|utf)-8").unwrap());

const ENABLED: &str = "# frozen_string_literal: true";

/// MagicComment#frozen_string_literal: Some("true"/"false"/other token, lowercased), None if unspecified.
pub fn frozen_value(line: &str) -> Option<String> {
    match EMACS.captures(line) {
        Some(e) => e[1].split(';').find_map(|t| EMACS_FROZEN.captures(t.trim()).map(|c| c[1].to_lowercase())),
        None => SIMPLE.captures(line).map(|c| c[1].to_lowercase()),
    }
}

/// `buffer.line_range(line)`: the line without its newline.
fn line_range(src: &Source, line: usize) -> (usize, usize) {
    let b = src.line_start(line);
    (b, b + src.text[b..].find('\n').unwrap_or(src.text.len() - b))
}

/// FrozenStringLiteralComment#insert_comment: after a leading shebang and/or encoding comment
/// (last_special_comment), else at the very top.
fn insert_comment(k: &mut Corrector, src: &Source, first_code: Option<usize>) {
    // Comment tokens before the first code token.
    let mut leading = src.comments.iter().filter(|c| first_code.is_none_or(|f| c.start < f));
    let mut special = None;
    let mut next = leading.next();
    if let Some(c) = next.filter(|c| src.slice(c.start, c.end).starts_with("#!")) {
        special = Some(c);
        next = leading.next();
    }
    if let Some(c) = next.filter(|c| ENCODING.is_match(src.slice(c.start, c.end))) {
        special = Some(c);
    }
    match special {
        Some(c) => {
            let (b, e) = line_range(src, src.line_of(c.start));
            k.insert_after(b, e, &format!("\n{ENABLED}"));
        }
        None => k.insert_before(0, src.text.len(), &format!("{ENABLED}\n")),
    }
}

impl Cop for FrozenStringLiteralComment {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.3, f64::MAX)
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let first_code = cx.facts.first_code;
        if helpers::parser_tokens::tokens_empty(src, first_code) {
            return;
        }
        let leading = helpers::frozen_string_literal::leading(cx);
        let values: Vec<String> = leading.iter().filter_map(|l| frozen_value(l)).collect();
        let valid = values.iter().any(|v| v == "true" || v == "false");
        // The first comment token anywhere that specifies the setting.
        let comment = src.comments.iter().find(|c| frozen_value(src.slice(c.start, c.end)).is_some());
        // Missing-comment offenses are on source_range(buffer, 0, 0), i.e. 0...1.
        match cx.cfg.str("EnforcedStyle").unwrap_or("always") {
            "never" => {
                if valid && let Some(c) = comment {
                    let (start, end) = (c.start, c.end);
                    cx.add_fix(start, end, "Unnecessary frozen string literal comment.", |k| {
                        let right = Surrounding { side: Side::Right, ..Default::default() };
                        let (b, e) = range_with_surrounding_space(src.text, start, end, right);
                        k.remove(b, e);
                    });
                }
            }
            "always_true" => {
                if values.is_empty() {
                    cx.add_fix(0, src.text.chars().next().map_or(0, char::len_utf8), "Missing magic comment `# frozen_string_literal: true`.", |k| {
                        insert_comment(k, src, first_code)
                    });
                } else if values[0] != "true"
                    && let Some(c) = comment
                {
                    let (start, end) = (c.start, c.end);
                    cx.add_fix(start, end, "Frozen string literal comment must be set to `true`.", |k| {
                        // enable_comment: MagicComment#new_frozen_string_literal(true) over the line.
                        let (b, e) = line_range(src, src.line_of(start));
                        let emacs = EMACS.is_match(src.slice(start, end));
                        k.replace(b, e, if emacs { "# -*- frozen_string_literal: true -*-" } else { ENABLED });
                    });
                }
            }
            _ => {
                if !valid {
                    cx.add_fix(0, src.text.chars().next().map_or(0, char::len_utf8), "Missing frozen string literal comment.", |k| insert_comment(k, src, first_code));
                }
            }
        }
    }
}
