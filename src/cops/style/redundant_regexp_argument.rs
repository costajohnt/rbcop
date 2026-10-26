use crate::helpers::alignment::enabled_cop_str;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantRegexpArgument";
pub static COP: RedundantRegexpArgument = RedundantRegexpArgument;
pub struct RedundantRegexpArgument;

const METHODS: &[&str] =
    &["byteindex", "byterindex", "gsub", "gsub!", "partition", "rpartition", "scan", "split", "start_with?", "sub", "sub!"];

/// `/\A(?:#{LITERAL_REGEX})+\Z/` with LITERAL_REGEX
/// `[\w\s\-,"'!#%&<>=;:\`~/]|\\[^AbBdDgGhHkpPRwWXsSzZ0-9]` (ASCII `\w`/`\s`).
fn deterministic(src: &str) -> bool {
    let src = src.strip_suffix('\n').unwrap_or(src);
    let mut it = src.chars();
    let mut any = false;
    while let Some(c) = it.next() {
        any = true;
        let plain = c.is_ascii_alphanumeric() || " \t\n\x0b\x0c\r_-,\"'!#%&<>=;:`~/".contains(c);
        if plain {
            continue;
        }
        if c != '\\' {
            return false;
        }
        match it.next() {
            Some(n) if !"AbBdDgGhHkpPRwWXsSzZ0123456789".contains(n) => {}
            _ => return false,
        }
    }
    any
}

const STR_SPECIAL_CHARS: &[&str] = &[
    "\\a", "\\c", "\\C", "\\e", "\\f", "\\M", "\\n", "\\\"", "\\'", "\\\\", "\\t", "\\b", "\\r", "\\u", "\\v", "\\x",
    "\\0", "\\1", "\\2", "\\3", "\\4", "\\5", "\\6", "\\7",
];

fn replacement(content: &str) -> String {
    let mut out = String::new();
    let mut pending = false;
    for c in content.chars() {
        if !pending && c == '\\' {
            pending = true;
            continue;
        }
        let piece = if pending { format!("\\{c}") } else { c.to_string() };
        pending = false;
        if STR_SPECIAL_CHARS.contains(&piece.as_str()) {
            out.push_str(&piece);
        } else {
            out.extend(piece.chars().filter(|&c| c != '\\'));
        }
    }
    out
}

/// `gsub(/(?<!\\)((?:\\\\)*)'/) { "#{$1}\\'" }`: escape each `'` behind an even backslash run.
fn escape_unescaped_quotes(s: &str) -> String {
    let mut out = String::new();
    let mut run = 0;
    for c in s.chars() {
        if c == '\'' && run % 2 == 0 {
            out.push('\\');
        }
        run = if c == '\\' { run + 1 } else { 0 };
        out.push(c);
    }
    out
}

fn preferred_argument(content: &str, double: bool) -> String {
    let mut arg = replacement(content);
    let quote = if arg.contains('"') {
        arg = arg.replace('\'', "\\'").replace("\\\"", "\"");
        '\''
    } else if arg.contains("\\'") {
        arg = escape_unescaped_quotes(&arg);
        '\''
    } else if arg.contains('\'') {
        arg = arg.replace('\'', "\\'");
        '\''
    } else if arg.contains('\\') || double {
        '"'
    } else {
        '\''
    };
    if quote == '"' {
        // `#{`, `#$` and `#@` would start an interpolation in a double-quoted string.
        let mut escaped = String::with_capacity(arg.len());
        let mut chars = arg.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '#' && chars.peek().is_some_and(|n| matches!(n, '{' | '$' | '@')) {
                escaped.push('\\');
            }
            escaped.push(c);
        }
        arg = escaped;
    }
    format!("{quote}{arg}{quote}")
}

impl Cop for RedundantRegexpArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !METHODS.contains(&method_name(&call)) {
            return;
        }
        let Some(first) = call.arguments().and_then(|a| a.arguments().iter().next()) else { return };
        let Some(re) = first.as_regular_expression_node() else { return };
        // regopt must be empty: the closing delimiter has no options.
        if re.closing_loc().as_slice().len() != 1 {
            return;
        }
        let content = text(&re.content_loc());
        let src = node_text(&first);
        if content == " " || !deterministic(src) {
            return;
        }
        let double = enabled_cop_str(cx, "Style/StringLiterals", "EnforcedStyle").as_deref() == Some("double_quotes");
        let prefer = preferred_argument(content, double);
        let msg = format!("Use string `{prefer}` as argument instead of regexp `{src}`.");
        let l = first.location();
        let (start, end) = (l.start_offset(), l.end_offset());
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &prefer));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_preferred_argument() {
        assert!(deterministic("/a\\.b/"));
        assert!(!deterministic("/a.b/"));
        assert!(!deterministic("/\\d/"));
        assert_eq!(preferred_argument("a\\.b", false), "'a.b'");
        assert_eq!(preferred_argument("a\\nb", false), "\"a\\nb\"");
        assert_eq!(preferred_argument("it's", false), "'it\\'s'");
        assert_eq!(preferred_argument("\"x'", false), "'\"x\\''");
        assert_eq!(preferred_argument("a", true), "\"a\"");
        assert_eq!(preferred_argument("a#{b", true), "\"a\\#{b\"");
    }
}
