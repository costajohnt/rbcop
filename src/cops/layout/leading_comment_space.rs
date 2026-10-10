use crate::prelude::*;

pub const NAME: &str = "Layout/LeadingCommentSpace";
pub static COP: LeadingCommentSpace = LeadingCommentSpace;
pub struct LeadingCommentSpace;

/// `/\A(?!#\+\+|#--)(#+[^#\s=])/`
fn missing_space(text: &str) -> bool {
    if text.starts_with("#++") || text.starts_with("#--") {
        return false;
    }
    let rest = text.trim_start_matches('#');
    rest.len() < text.len()
        && rest.chars().next().is_some_and(|c| !matches!(c, ' ' | '\t' | '\r' | '\n' | '\x0c' | '\x0b' | '='))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

static RBS_BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#\[.+\]").unwrap());
static YARD_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A#-\s*\z").unwrap());

impl Cop for LeadingCommentSpace {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        let flag = |k: &str| cx.cfg.get(k).is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)));
        let (doxygen, gemfile_ruby, rbs, steep, yard) = (
            flag("AllowDoxygenCommentStyle"),
            flag("AllowGemfileRubyComment"),
            flag("AllowRBSInlineAnnotation"),
            flag("AllowSteepAnnotation"),
            flag("AllowYARDCommentBlockSeparator"),
        );
        let file = basename(src.path);
        // Starts of comments reported so far (current_offense_locations).
        let mut offended: Vec<usize> = Vec::new();
        for c in &src.comments {
            let text = src.slice(c.start, c.end);
            if !missing_space(text) {
                continue;
            }
            let line = src.line_of(c.start);
            let shebang = text.starts_with("#!");
            if line == 1 && (shebang || (file == "config.ru" && text.starts_with("#\\"))) {
                continue;
            }
            // shebang_continuation?
            if shebang
                && src.comment_on_line(line - 1).is_some_and(|p| {
                    src.slice(p.start, p.end).starts_with("#!") && !offended.contains(&p.start)
                })
            {
                continue;
            }
            if (doxygen && text.starts_with("#*"))
                || (gemfile_ruby && file == "Gemfile" && text.starts_with("#ruby"))
                || (rbs && (text.starts_with("#:") || text.starts_with("#|") || RBS_BRACKET.is_match(text)))
                || (steep && (text.starts_with("#$") || text.starts_with("#:")))
                || (yard && YARD_SEPARATOR.is_match(text))
            {
                continue;
            }
            offended.push(c.start);
            cx.add(c.start, c.end, "Missing space after `#`.");
        }
    }
}
