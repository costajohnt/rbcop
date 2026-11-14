//! rubocop-sorbet's ValidSigil, which the other sigil cops (HasSigil and its strictness subclasses,
//! EnforceSigilOrder, EnforceSingleSigil) extend: the comment tokens before the first code token
//! and the `# typed:` sigil among them.

use crate::config::validator::to_s;
use crate::prelude::*;
use crate::source::Source;

pub const STRICTNESS_LEVELS: [&str; 5] = ["ignore", "false", "true", "strict", "strong"];

/// ValidSigil::SIGIL_REGEX: Ruby's `[[:blank:]]` is Unicode (tab and Zs), its `\S` any character
/// but ASCII whitespace.
pub static SIGIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[\t\p{Zs}]*(?:#[\t\p{Zs}]*)?#[\t\p{Zs}]+typed:(?:[\t\p{Zs}]+([^ \t\r\n\x0b\x0c]+))?").unwrap()
});

/// `processed_source.tokens.take_while { |token| token.type == :tCOMMENT }`: their ranges.
pub fn leading_comments(src: &Source) -> Vec<(usize, usize)> {
    helpers::parser_tokens::translated(src)
        .iter()
        .take_while(|t| t.kind == sys::PM_TOKEN_COMMENT)
        .map(|t| (t.start, t.end))
        .collect()
}

/// Which ValidSigil (sub)class runs: HasSigil and its subclasses always require a sigil, and the
/// strictness subclasses (FalseSigil, ...) fix `minimum_strictness`.
#[derive(Clone, Copy)]
pub enum Kind {
    Valid,
    Has,
    Minimum(&'static str),
}

/// `cop_config[key].to_s`, if one of STRICTNESS_LEVELS.
fn level(cx: &Cx, key: &str) -> Option<&'static str> {
    let s = cx.cfg.params.get(key).map(to_s).unwrap_or_default();
    STRICTNESS_LEVELS.iter().copied().find(|l| *l == s)
}

fn index(level: &str) -> usize {
    STRICTNESS_LEVELS.iter().position(|l| *l == level).unwrap()
}

/// ValidSigil#on_new_investigation.
pub fn investigate(cx: &mut Cx, kind: Kind) {
    let src = cx.src;
    if helpers::parser_tokens::tokens_empty(src, cx.facts.first_code) {
        return;
    }
    let require_all = match kind {
        Kind::Valid => cx.cfg.bool("RequireSigilOnAllFiles").unwrap_or(false),
        _ => true,
    };
    let minimum = match kind {
        Kind::Minimum(m) => Some(m),
        _ => level(cx, "MinimumStrictness"),
    };
    let exact = level(cx, "ExactStrictness");
    let suggested = level(cx, "SuggestedStrictness").unwrap_or("false");
    let suggested_level = if let Some(e) = exact {
        e
    } else if let Some(m) = minimum {
        if m == "ignore" && matches!(cx.cfg.params.get("SuggestedStrictness"), None | Some(Value::Null)) {
            "ignore"
        } else {
            STRICTNESS_LEVELS[index(suggested).max(index(m))]
        }
    } else {
        suggested
    };

    let sigil = leading_comments(src).into_iter().find(|&(s, e)| SIGIL.is_match(&src.text[s..e]));
    let Some((start, end)) = sigil else {
        // check_sigil_present: the offense is on the first token; ValidSigil#autocorrect inserts
        // the suggested sigil (after a shebang line).
        if require_all && let Some(first) = helpers::parser_tokens::translated(src).first().copied() {
            let msg = format!(
                "No Sorbet sigil found in file. Try a `typed: {suggested_level}` to start (you can also use `rubocop -a` to automatically add this)."
            );
            let sigil = format!("# typed: {suggested_level}");
            let shebang = src.text[first.start..first.end].starts_with("#!");
            cx.add_fix(first.start, first.end, msg, |k| {
                if shebang {
                    k.insert_after(first.start, first.end, &format!("\n{sigil}"));
                } else {
                    k.insert_before(first.start, first.end, &format!("{sigil}\n"));
                }
            });
        }
        return;
    };
    let text = &src.text[start..end];
    let strictness = SIGIL.captures(text).and_then(|c| c.get(1)).map(|m| m.as_str());

    // check_double_commented_sigil
    static DOUBLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\t\p{Zs}]*#[\t\p{Zs}]*#").unwrap());
    if DOUBLE.is_match(text) {
        let fixed = format!("# typed: {}", strictness.unwrap_or(""));
        cx.add_fix(start, end, format!("Invalid Sorbet sigil `{text}`."), |k| k.replace(start, end, &fixed));
    }
    // The other offenses' corrector is ValidSigil#autocorrect, which only adds a missing sigil.
    let Some(strictness) = strictness else {
        cx.add_with(start, end, "Sorbet sigil should not be empty.", false);
        return;
    };
    if !STRICTNESS_LEVELS.contains(&strictness) {
        cx.add_with(start, end, format!("Invalid Sorbet sigil `{strictness}`."), false);
        return;
    }
    let current = index(strictness);
    if let Some(e) = exact {
        if current != index(e) {
            cx.add_with(start, end, format!("Sorbet sigil should be `{e}` got `{strictness}`."), false);
        }
    } else if let Some(m) = minimum
        && current < index(m)
    {
        cx.add_with(start, end, format!("Sorbet sigil should be at least `{m}` got `{strictness}`."), false);
    }
}
