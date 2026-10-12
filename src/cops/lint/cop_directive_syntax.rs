use crate::directives::{self, DIRECTIVE};
use crate::helpers::cop_registry::{REGISTRY, is_department, qualified_cop_name};
use crate::prelude::*;

pub const NAME: &str = "Lint/CopDirectiveSyntax";
pub static COP: CopDirectiveSyntax = CopDirectiveSyntax;
pub struct CopDirectiveSyntax;

const COMMON_MSG: &str = "Malformed directive comment detected.";
const MISSING_MODE_NAME_MSG: &str = "The mode name is missing.";
const INVALID_MODE_NAME_MSG: &str = "The mode name must be one of `enable`, `disable`, `disable-next`, `enable-next`, `todo`, `todo-next`, `next`, `push`, or `pop`.";
const MISSING_COP_NAME_MSG: &str = "The cop name is missing.";
const MULTIPLE_DIRECTIVES_MSG: &str =
    "Only the first directive on a line takes effect. List the cop names in a single directive instead.";
const MALFORMED_COP_NAMES_MSG: &str =
    "Cop names must be separated by commas. Comment in the directive must start with `--`.";
const INVALID_SIGNED_ARGS_MSG: &str =
    "`push` and `next` arguments must be `+`- or `-`-prefixed cop names, and `pop` takes no arguments.";
const NEXT_DIRECTIVE_AT_EOL_MSG: &str = "A `-next` directive must be on its own line, above the statement it applies to.";
const AVAILABLE_MODES: &[&str] = &["disable", "enable", "todo", "push", "pop", "disable-next", "todo-next", "enable-next", "next"];
const MODES: &str = "disable-next|enable-next|todo-next|disable|enable|todo|push|next|pop";

/// Ruby's whitespace for `lstrip`, `split(' ')` and `\s` (ASCII only; `char::is_whitespace` is Unicode).
const RUBY_SPACE: [char; 6] = [' ', '\t', '\n', '\x0b', '\x0c', '\r'];

use crate::helpers::ruby_string::strip as ruby_strip;

/// A Ruby pattern (ASCII `\s`, `\w`).
fn ruby_re(pattern: &str) -> Regex {
    Regex::new(&crate::helpers::ruby_string::ruby_regex_classes(pattern)).unwrap()
}

static MARKER: LazyLock<Regex> =LazyLock::new(|| ruby_re(r"#\s*rubocop\s*:\s*"));
static MARKER_START: LazyLock<Regex> = LazyLock::new(|| ruby_re(r"\A#\s*rubocop\s*:\s*"));
static WITHOUT_COP_NAME: LazyLock<Regex> =
    LazyLock::new(|| ruby_re(&format!(r"\A#\s*rubocop\s*:\s*(?:{MODES})\b\s*\z")));
static NEAR_MISS_KEYWORD: LazyLock<Regex> =
    LazyLock::new(|| ruby_re(&format!(r"\A#+\s*([A-Za-z][\w-]*)\s*:\s*(?:{MODES})\b")));

/// Ruby `str.split(/,\s*/)` / `split(sep)`: trailing empty fields dropped.
fn ruby_split<'a>(s: &'a str, re: &Regex) -> Vec<&'a str> {
    let mut v: Vec<&str> = re.split(s).collect();
    while v.last().is_some_and(|x| x.is_empty()) {
        v.pop();
    }
    v
}

impl Cop for CopDirectiveSyntax {
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
            let (start, end) = (c.start, c.start + text.len());
            if MARKER_START.is_match(text) {
                if check_directive(cx, start, end, text).is_err() {
                    return;
                }
            } else if let Some(k) = NEAR_MISS_KEYWORD.captures(text) {
                let keyword = &k[1];
                let lower = keyword.to_lowercase();
                if keyword != "rubocop"
                    && (lower == "rubocop" || helpers::did_you_mean::levenshtein(&lower, "rubocop") <= 2)
                {
                    let msg = format!("{COMMON_MSG} The directive keyword must be `rubocop`, not `{keyword}`.");
                    cx.add_with(start, end, msg, false);
                }
            }
        }
    }
}

/// Err: the cop raised (AmbiguousCopName) and RuboCop reports nothing more for the file.
fn check_directive(cx: &mut Cx, start: usize, end: usize, text: &str) -> Result<(), ()> {
    let directive = directives::parse(text);
    let mode = directive.as_ref().map(|d| d.mode.as_str());
    let cops = directive.as_ref().and_then(|d| d.cops.as_deref());
    let tail = directive.as_ref().map(|d| text[d.end..].trim_start_matches(RUBY_SPACE));
    let reason = tail.filter(|t| t.starts_with("--")).is_some_and(|t| !ruby_strip(&t[2..]).is_empty());

    // multiple_directives
    let mut matches: Vec<regex::Captures> = DIRECTIVE.captures_iter(text).collect();
    if reason && matches.len() > 1 {
        let first = matches.remove(0);
        matches.retain(|m| text[m.get(0).unwrap().end()..].chars().all(|c| RUBY_SPACE.contains(&c)));
        matches.insert(0, first);
    }
    if matches.len() > 1 {
        let replacement = merged(text, &matches);
        let msg = format!("{COMMON_MSG} {MULTIPLE_DIRECTIVES_MSG}");
        cx.add_fix(start, end, msg, |c| {
            if let Some(r) = replacement {
                c.replace(start, end, &r);
            }
        });
        return Ok(());
    }

    let push = mode == Some("push");
    let pop = mode == Some("pop");
    let signed = push || mode == Some("next");
    let missing_cop_name = !push && !pop && WITHOUT_COP_NAME.is_match(text);
    let invalid_signed_args = if pop {
        cops.is_some_and(|c| !c.is_empty())
    } else {
        signed && cops.is_some_and(|c| c.split_whitespace().any(|s| !s.starts_with(['+', '-'])))
    };
    let malformed = directive.is_none()
        || missing_cop_name
        || invalid_signed_args
        || !tail.is_some_and(|t| t.is_empty() || t.starts_with("--"));
    if malformed {
        let after_marker = MARKER.replace(text, "");
        let extra = match after_marker.split(RUBY_SPACE).find(|s| !s.is_empty()) {
            None => MISSING_MODE_NAME_MSG,
            Some(m) if !AVAILABLE_MODES.contains(&m) => INVALID_MODE_NAME_MSG,
            _ if missing_cop_name => MISSING_COP_NAME_MSG,
            _ if invalid_signed_args => INVALID_SIGNED_ARGS_MSG,
            _ => MALFORMED_COP_NAMES_MSG,
        };
        cx.add_with(start, end, format!("{COMMON_MSG} {extra}"), false);
        return Ok(());
    }
    let next_statement = matches!(mode, Some("disable-next" | "todo-next" | "enable-next" | "next"));
    // comment_only_line?: no code token on the line (a BOM or other non-token text before is fine).
    if next_statement && helpers::comment_config::code_lines(cx.src, cx.facts)[cx.src.line_of(start)] {
        cx.add_with(start, end, NEXT_DIRECTIVE_AT_EOL_MSG, false);
        return Ok(());
    }
    // unknown_cop_name
    if pop || cops == Some("all") {
        return Ok(());
    }
    let names: Vec<&str> = if signed {
        // signed_args: a hash keyed by op in order of first appearance, values flattened.
        let specs: Vec<&str> = cops.unwrap_or("").split_whitespace().collect();
        let mut ops: Vec<char> = Vec::new();
        for s in &specs {
            let op = s.chars().next().unwrap();
            if matches!(op, '+' | '-') && !ops.contains(&op) {
                ops.push(op);
            }
        }
        ops.iter().flat_map(|op| specs.iter().filter(move |s| s.starts_with(*op)).map(|s| &s[1..])).collect()
    } else {
        static COMMA: LazyLock<Regex> = LazyLock::new(|| ruby_re(r",\s*"));
        ruby_split(cops.unwrap_or(""), &COMMA)
    };
    for name in names {
        if is_department(name) {
            continue;
        }
        let qualified = qualified_cop_name(name).ok_or(())?;
        if crate::helpers::cop_registry::is_known(&qualified) || is_department(&qualified) {
            continue;
        }
        let dictionary: Vec<&str> = REGISTRY.iter().copied().filter(|n| *n != name).collect();
        let suggestion = helpers::did_you_mean::correct(name, &dictionary)
            .first()
            .map_or(String::new(), |s| format!(" (did you mean `{s}`?)"));
        cx.add_with(start, end, format!("Unknown cop name `{name}`{suggestion}."), false);
        break;
    }
    Ok(())
}

/// merge_directives: the single directive replacing adjacent ones, if they can be merged.
fn merged(text: &str, directives: &[regex::Captures]) -> Option<String> {
    // directives_adjacent?
    for (i, m) in directives.iter().enumerate() {
        let from = m.get(0).unwrap().end();
        let to = directives.get(i + 1).map_or(text.len(), |n| n.get(0).unwrap().start());
        if !ruby_strip(&text[from..to]).is_empty() {
            return None;
        }
    }
    // merged_mode
    let mut modes: Vec<&str> = Vec::new();
    for m in directives {
        let mode = m.get(1).unwrap().as_str();
        if !modes.contains(&mode) {
            modes.push(mode);
        }
    }
    if modes.len() > 1 && !modes.iter().all(|m| matches!(*m, "disable" | "todo")) {
        return None;
    }
    // merged_cops
    let mut cops: Vec<&str> = Vec::new();
    for m in directives {
        for c in m.get(2).map_or("", |g| g.as_str()).split(',').map(str::trim) {
            if !c.is_empty() && !cops.contains(&c) {
                cops.push(c);
            }
        }
    }
    if cops.is_empty() || cops.contains(&"all") {
        return None;
    }
    Some(format!("# rubocop:{} {}", modes[0], cops.join(", ")))
}
