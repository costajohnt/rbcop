use crate::helpers::magic_comment;
use crate::prelude::*;

pub const NAME: &str = "Style/MagicCommentFormat";
pub static COP: MagicCommentFormat = MagicCommentFormat;
pub struct MagicCommentFormat;

const DIRECTIVE: &str = r"(?i:(?:en)?coding)|(?i:frozen[_-]string[_-]literal)|(?i:rbs_inline)|(?i:warn[_-]indent)|(?i:shareable[_-]constant[_-]value)|(?i:typed)";
static DIRECTIVE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(DIRECTIVE).unwrap());
// `(?:DIRECTIVE:\s*)(.*?)(?=;|$)`; the `\s` sits in a double-quoted Ruby string, so it is a space.
static VALUE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"(?:{DIRECTIVE}): *([^;\n]*)")).unwrap());

#[derive(Clone, Copy, PartialEq)]
enum Case {
    Lower,
    Upper,
}

fn capitalization(cx: &Cx, key: &str) -> Option<Case> {
    match cx.cfg.str(key)? {
        "lowercase" => Some(Case::Lower),
        "uppercase" => Some(Case::Upper),
        _ => None,
    }
}

fn wrong_case(text: &str, case: Option<Case>) -> bool {
    match case {
        Some(Case::Lower) => text != text.to_lowercase(),
        Some(Case::Upper) => text != text.to_uppercase(),
        None => false,
    }
}

fn recase(text: &str, case: Option<Case>) -> String {
    match case {
        Some(Case::Lower) => text.to_lowercase(),
        Some(Case::Upper) => text.to_uppercase(),
        None => text.to_string(),
    }
}

impl Cop for MagicCommentFormat {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        // `return unless processed_source.ast`; leading comments are those on lines before the
        // first non-comment token.
        let Some(first_code) = cx.facts.first_code else { return };
        let src = cx.src;
        let first_line = src.line_of(first_code);
        let snake = cx.cfg.str("EnforcedStyle") != Some("kebab_case");
        let (wrong_sep, right_sep) = if snake { ('-', '_') } else { ('_', '-') };
        let directive_case = capitalization(cx, "DirectiveCapitalization");
        let value_case = capitalization(cx, "ValueCapitalization");
        let dir_word = match directive_case {
            Some(Case::Lower) => "lower ",
            Some(Case::Upper) => "upper ",
            None => "",
        };
        let msg = format!("Prefer {dir_word}{} case for magic comments.", if snake { "snake" } else { "kebab" });
        let value_msg = format!(
            "Prefer {} for magic comment values.",
            match value_case {
                Some(Case::Lower) => "lowercase",
                Some(Case::Upper) => "uppercase",
                None => "",
            }
        );
        for c in src.comments.iter().filter(|c| src.line_of(c.start) < first_line) {
            let text = &src.text[c.start..c.end];
            if !magic_comment::valid(text) {
                continue;
            }
            let directives: Vec<(usize, usize)> = DIRECTIVE_RE
                .find_iter(text)
                .map(|m| (c.start + m.start(), c.start + m.end()))
                .filter(|&(s, e)| {
                    let d = &src.text[s..e];
                    d.contains(wrong_sep) || wrong_case(d, directive_case)
                })
                .collect();
            let values: Vec<(usize, usize)> = VALUE_RE
                .captures_iter(text)
                .filter_map(|m| m.get(1))
                .map(|m| (c.start + m.start(), c.start + m.end()))
                .filter(|&(s, e)| wrong_case(&src.text[s..e], value_case))
                .collect();
            for (s, e) in directives {
                let fixed = recase(&src.text[s..e], directive_case).replace(wrong_sep, &right_sep.to_string());
                cx.add_fix(s, e, msg.as_str(), |k| k.replace(s, e, &fixed));
            }
            for (s, e) in values {
                let fixed = recase(&src.text[s..e], value_case);
                cx.add_fix(s, e, value_msg.as_str(), |k| k.replace(s, e, &fixed));
            }
        }
    }
}
