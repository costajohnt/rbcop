use crate::helpers::cop_registry;
use crate::prelude::*;

pub const NAME: &str = "Migration/DepartmentName";
pub static COP: DepartmentName = DepartmentName;
pub struct DepartmentName;

/// DISABLE_COMMENT_FORMAT's prefix (group 1): exact spaces, unlike the general directive pattern.
static PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A# *rubocop *: *(?:(?:dis|en)able|todo) +").unwrap());
/// `scan(/[^,]+|\W+/)`
static TOKENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^,]+|[^0-9A-Za-z_]+").unwrap());
/// DISABLING_COPS_CONTENT_TOKEN (unanchored: `MethodCallParentheses` contains `all`).
static CONTENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z]+/[A-Za-z]+|all").unwrap());
static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^0-9A-Za-z_]").unwrap());
static UNEXPECTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z/, ]").unwrap());

impl Cop for DepartmentName {
    fn correctable(&self) -> bool {
        true
    }

    fn on_file(&self, cx: &mut Cx) {
        let src = cx.src;
        for c in &src.comments {
            if c.embdoc {
                continue;
            }
            let text = src.slice(c.start, c.end).trim_end_matches('\n');
            let directive = crate::directives::parse(text)
                .filter(|d| d.start == 0 && matches!(d.mode.as_str(), "disable" | "enable" | "todo"));
            let Some(prefix) = directive.and_then(|_| PREFIX.find(text)) else { continue };
            let mut offset = prefix.end();
            for m in TOKENS.find_iter(&text[offset..]) {
                let name = m.as_str();
                let trimmed = helpers::ruby_string::strip(name);
                let valid =
                    NON_WORD.is_match(trimmed) || CONTENT.is_match(trimmed) || cop_registry::is_department(trimmed);
                if !valid {
                    check_cop_name(cx, trimmed, c.start + offset);
                }
                if UNEXPECTED.is_match(name) {
                    break;
                }
                offset += name.len();
            }
        }
    }
}

fn check_cop_name(cx: &mut Cx, name: &str, start: usize) {
    let end = start + name.len();
    let qualified = match cop_registry::qualified_cop_name(name) {
        Some(q) if q.contains('/') => Some(q),
        // qualified_legacy_cop_name: ConfigObsoletion.legacy_cop_names, the loaded plugins' too
        Some(_) => crate::config::validator::legacy_cop_names().into_iter().find(|l| l.split('/').nth(1) == Some(name)),
        None => None,
    };
    // A nil replacement raises inside the corrector: the offense stays, without a correction.
    cx.add_fix(start, end, "Department name is missing.", |c| {
        if let Some(q) = qualified {
            c.replace(start, end, &q);
        }
    });
}
