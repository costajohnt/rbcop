use crate::prelude::*;

pub const NAME: &str = "Style/RedundantPercentQ";
pub static COP: RedundantPercentQ = RedundantPercentQ;
pub struct RedundantPercentQ;

static STRING_INTERPOLATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#\{.+\}").unwrap());

/// Util#double_quotes_required?: `/'|(?<! \\) \\{2}* \\ (?![\\"])/x`, i.e. a single quote, or a
/// maximal run of an odd number of backslashes not followed by `"`.
fn double_quotes_required(s: &str) -> bool {
    let b = s.as_bytes();
    if b.contains(&b'\'') {
        return true;
    }
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i] == b'\\' {
            i += 1;
        }
        if (i - start) % 2 == 1 && b.get(i) != Some(&b'"') {
            return true;
        }
    }
    false
}

/// `src.scan(/\\./).any?(/\\[^\\]/)`: an escaped character other than a backslash (`.` skips newlines).
fn escaped_non_backslash(s: &str) -> bool {
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('\n') | None => {}
                Some('\\') => {
                    chars.next();
                }
                Some(_) => return true,
            }
        }
    }
    false
}

impl Cop for RedundantPercentQ {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE, kind::INTERPOLATED_STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (open, close) = if let Some(s) = node.as_string_node() {
            (s.opening_loc(), s.closing_loc())
        } else {
            let s = node.as_interpolated_string_node().unwrap();
            (s.opening_loc(), s.closing_loc())
        };
        let (Some(open), Some(close)) = (open, close) else { return };
        let src = node_text(node);
        let capital = src.starts_with("%Q");
        if !src.starts_with("%q") && !capital {
            return;
        }
        if src.contains('\'') && src.contains('"') {
            return;
        }
        let allowed = if capital {
            src.contains('"')
                && (STRING_INTERPOLATION.is_match(src) || (!is_parser_dstr(node) && node.as_string_node().is_some() && double_quotes_required(src)))
        } else {
            (STRING_INTERPOLATION.is_match(src) && src.contains('\'')) || escaped_non_backslash(src)
        };
        if allowed {
            return;
        }
        let extra = if capital { ", or for dynamic strings that contain double quotes" } else { "" };
        let msg = format!(
            "Use `{}` only for strings that contain both single quotes and double quotes{extra}.",
            &src[..2]
        );
        let delimiter = if (capital && !src.contains('"')) || src.contains('\'') { "\"" } else { "'" };
        let loc = node.location();
        let (o, c) = ((open.start_offset(), open.end_offset()), (close.start_offset(), close.end_offset()));
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |cr| {
            cr.replace(o.0, o.1, delimiter);
            cr.replace(c.0, c.1, delimiter);
        });
    }
}
