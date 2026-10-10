use crate::config::value::ruby_regex;
use crate::helpers::parser_tokens::translated;
use crate::prelude::*;

pub const NAME: &str = "Style/Copyright";
pub static COP: Copyright = Copyright;
pub struct Copyright;

static NOTICE_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?:\\A|\^)?#(?:\\s[*+?]?|[ \t\r\n\f\v])*").unwrap());
static COMMENT_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A# *").unwrap());
static LINE_COMMENT_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^# *").unwrap());
static SHEBANG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#!.*\z").unwrap());
static ENCODING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A#.*coding[ \t\r\n\f\v]?[:=][ \t\r\n\f\v]?(?:UTF|utf)-8").unwrap());

/// `normalized_autocorrect_notice`
fn normalized(notice: &str) -> String {
    notice
        .split_inclusive('\n')
        .map(|line| {
            if line.starts_with('#') {
                line.to_string()
            } else if line.strip_suffix('\n').unwrap_or(line).is_empty() {
                "#\n".to_string()
            } else {
                format!("# {line}")
            }
        })
        .collect()
}

impl Cop for Copyright {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let notice = cx.cfg.str("Notice").unwrap_or("").to_string();
        if notice.is_empty() {
            return;
        }
        // ponytail: a Notice Ruby compiles but the regex crate rejects reports no offense
        let Some(re) = ruby_regex(&NOTICE_PREFIX.replace(&notice, "")) else { return };
        let src = cx.src;
        // notice_found?: the comment tokens before the first code token
        let mut multiline = String::new();
        for c in &src.comments {
            if cx.facts.first_code.is_some_and(|f| c.start >= f) {
                break;
            }
            let text = src.slice(c.start, c.end).trim_end_matches('\n');
            multiline.push_str(&COMMENT_PREFIX.replace(text, ""));
            multiline.push('\n');
            if re.is_match(text) {
                break;
            }
        }
        if re.is_match(&multiline) {
            return;
        }
        let msg = format!("Include a copyright notice matching /{notice}/ before any code.");
        if cx.facts.first_code.is_none() {
            return cx.add_global(msg);
        }
        let end = src.text.chars().next().map_or(0, char::len_utf8);
        if cx.fixes.autocorrect {
            // verify_autocorrect_notice! raises a Warning, which loses the offense
            let ac = cx.cfg.str("AutocorrectNotice").unwrap_or("");
            let problem = if ac.is_empty() {
                Some("An AutocorrectNotice must be defined in your RuboCop config".to_string())
            } else if !re.is_match(&LINE_COMMENT_PREFIX.replace_all(&normalized(ac), "")) {
                Some(format!("AutocorrectNotice '{ac}' must match Notice /{notice}/"))
            } else {
                None
            };
            if let Some(p) = problem {
                if !crate::directives::suppressed(&cx.facts.disabled, cx.cop, 1, 1) {
                    eprintln!("{NAME}: {p} (from file: {})", src.path);
                    return;
                }
                return cx.add(0, end, msg);
            }
            let insert = format!("{}\n", normalized(ac));
            // insert_notice_before: after a shebang and an encoding comment
            let tokens = translated(src);
            let is_comment = |i: usize| {
                tokens.get(i).filter(|t| matches!(t.kind, sys::PM_TOKEN_COMMENT | sys::PM_TOKEN_EMBDOC_BEGIN))
                    .map(|t| src.slice(t.start, t.end).trim_end_matches('\n'))
            };
            let mut idx = 0;
            if is_comment(idx).is_some_and(|t| SHEBANG.is_match(t)) {
                idx += 1;
            }
            if is_comment(idx).is_some_and(|t| ENCODING.is_match(t)) {
                idx += 1;
            }
            let range = tokens.get(idx).map_or((0, 0), |t| (t.start, t.end));
            return cx.add_fix(0, end, msg, |c| c.insert_before(range.0, range.1, &insert));
        }
        cx.add_fix(0, end, msg, |_| {});
    }
}
