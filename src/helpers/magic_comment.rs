//! RuboCop's MagicComment (`MagicComment.parse(text).any?`).

use crate::helpers::ruby_string::strip as ruby_strip;
use regex::Regex;
use std::sync::LazyLock;

// Ruby's `\s` is ASCII-only and `[[:alnum:]]` is Unicode-aware on UTF-8 strings.
const TOKEN: &str = r"([\p{Alphabetic}\p{Nd}\-_]+)";
const S: &str = r"[ \t\r\n\x0c\x0b]";

static EMACS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-\*-(.+)-\*-").unwrap());
static VIM: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"#{S}*vim:{S}*(.+)")).unwrap());
static EMACS_KEYS: LazyLock<Regex> = LazyLock::new(|| {
    let kw = r"(?:(?:en)?coding|frozen[_-]string[_-]literal|warn[_-]indent|shareable[_-]constant[_-]value)";
    Regex::new(&format!(r"\A{kw}{S}*:{S}*{TOKEN}\z")).unwrap()
});
static VIM_ENCODING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\Afileencoding{S}*={S}*{TOKEN}\z")).unwrap());
static SIMPLE: LazyLock<Regex> = LazyLock::new(|| {
    let kw = r"(?:frozen[_-]string[_-]literal|warn[_-]indent|shareable[_-]constant[_-]value|typed)";
    Regex::new(&format!(r"(?i)\A{S}*#{S}*{kw}:{S}*{TOKEN}{S}*\z")).unwrap()
});
static SIMPLE_RBS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\A{S}*#{S}*rbs_inline:{S}*{TOKEN}{S}*\z")).unwrap());
static SIMPLE_ENCODING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\A{S}*#{S}*(?:frozen_string_literal:{S}*(?:true|false))?{S}*(?:en)?coding: {TOKEN}"))
        .unwrap()
});

/// `MagicComment.parse(comment).any?`: the comment sets encoding, frozen_string_literal,
/// rbs_inline, warn_indent, shareable_constant_value or typed.
pub fn any(comment: &str) -> bool {
    if let Some(c) = EMACS.captures(comment) {
        return c[1].split(';').any(|t| EMACS_KEYS.is_match(ruby_strip(t)));
    }
    if let Some(c) = VIM.captures(comment) {
        let mut parts: Vec<&str> = c[1].split(", ").collect();
        while parts.last() == Some(&"") {
            parts.pop(); // Ruby's split drops trailing empty fields
        }
        let tokens: Vec<&str> = parts.into_iter().map(ruby_strip).collect();
        return tokens.len() > 1 && tokens.iter().any(|t| VIM_ENCODING.is_match(t));
    }
    SIMPLE.is_match(comment)
        || SIMPLE_ENCODING.is_match(comment)
        || SIMPLE_RBS.captures(comment).is_some_and(|c| matches!(&c[1], "enabled" | "disabled"))
}

static EMACS_ENCODING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A(?:en)?coding{S}*:{S}*{TOKEN}\z")).unwrap());

/// `MagicComment.parse(comment).encoding_specified?`.
pub fn encoding_specified(comment: &str) -> bool {
    if let Some(c) = EMACS.captures(comment) {
        return c[1].split(';').any(|t| EMACS_ENCODING.is_match(ruby_strip(t)));
    }
    if let Some(c) = VIM.captures(comment) {
        let mut parts: Vec<&str> = c[1].split(", ").collect();
        while parts.last() == Some(&"") {
            parts.pop();
        }
        return parts.len() > 1 && parts.into_iter().any(|t| VIM_ENCODING.is_match(ruby_strip(t)));
    }
    SIMPLE_ENCODING.is_match(comment)
}

/// `MagicComment.parse(comment).valid?`.
pub fn valid(comment: &str) -> bool {
    comment.starts_with('#') && any(comment)
}

/// Ruby's `split(sep)` minus trailing empty fields, each field stripped (EditorComment#tokens).
fn editor_tokens<'a>(body: &'a str, sep: &str) -> Vec<&'a str> {
    let mut parts: Vec<&str> = body.split(sep).collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts.into_iter().map(ruby_strip).collect()
}

/// `MagicComment.parse(comment).encoding`: the encoding value (editor comments downcase it).
pub fn encoding(comment: &str) -> Option<String> {
    if let Some(c) = EMACS.captures(comment) {
        let tokens = editor_tokens(c.get(1).unwrap().as_str(), ";");
        return tokens.iter().find_map(|t| EMACS_ENCODING.captures(t).map(|m| m[1].to_lowercase()));
    }
    if let Some(c) = VIM.captures(comment) {
        let tokens = editor_tokens(c.get(1).unwrap().as_str(), ", ");
        if tokens.len() <= 1 {
            return None;
        }
        return tokens.iter().find_map(|t| VIM_ENCODING.captures(t).map(|m| m[1].to_lowercase()));
    }
    SIMPLE_ENCODING.captures(comment).map(|m| m[1].to_string())
}

static SIMPLE_ENCODING_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)\A#{S}*(?:en)?coding")).unwrap());

/// `MagicComment.parse(comment).without(:encoding)`.
pub fn without_encoding(comment: &str) -> String {
    if let Some(c) = EMACS.captures(comment) {
        let tokens = editor_tokens(c.get(1).unwrap().as_str(), ";");
        let rest: Vec<&str> = tokens.into_iter().filter(|t| !t.starts_with("coding") && !t.starts_with("encoding")).collect();
        return if rest.is_empty() { String::new() } else { format!("# -*- {} -*-", rest.join(";")) };
    }
    if let Some(c) = VIM.captures(comment) {
        let tokens = editor_tokens(c.get(1).unwrap().as_str(), ", ");
        let rest: Vec<&str> = tokens.into_iter().filter(|t| !t.starts_with("fileencoding")).collect();
        return if rest.is_empty() { String::new() } else { format!("# vim: {}", rest.join(", ")) };
    }
    if SIMPLE_ENCODING_START.is_match(comment) { String::new() } else { comment.to_string() }
}

static EMACS_SHAREABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\Ashareable[_-]constant[_-]value{S}*:{S}*{TOKEN}\z")).unwrap());
static SIMPLE_SHAREABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\A{S}*#{S}*shareable[_-]constant[_-]value:{S}*{TOKEN}{S}*\z")).unwrap()
});

/// `MagicComment.parse(comment).shareable_constant_value` (vim comments never set it).
pub fn shareable_constant_value(comment: &str) -> Option<String> {
    if let Some(c) = EMACS.captures(comment) {
        let tokens = editor_tokens(c.get(1).unwrap().as_str(), ";");
        return tokens.iter().find_map(|t| EMACS_SHAREABLE.captures(t).map(|m| m[1].to_lowercase()));
    }
    if VIM.is_match(comment) {
        return None;
    }
    SIMPLE_SHAREABLE.captures(comment).map(|m| m[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::any;

    #[test]
    fn recognises_magic_comments() {
        for yes in [
            "# frozen_string_literal: true",
            "# -*- coding: utf-8 -*-",
            "# vim: fileencoding=utf-8, x=y",
            "# rbs_inline: enabled",
            "# typed: strict",
            "# encoding: ascii",
        ] {
            assert!(any(yes), "{yes}");
        }
        for no in ["# hello", "# vim: fileencoding=utf-8", "# rbs_inline: maybe", "# -*- Frozen_String_Literal: true -*-"] {
            assert!(!any(no), "{no}");
        }
    }
}
