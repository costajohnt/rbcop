use crate::config::value::ruby_regex;
use crate::helpers::percent_array as pa;
use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Style/WordArray";
pub static COP: WordArray = WordArray;
pub struct WordArray;

const PERCENT_MSG: &str = "Use `%w` or `%W` for an array of words.";

/// `str_content`: the value of a parser `str` node.
fn str_content(node: &Node) -> Option<Vec<u8>> {
    node.as_string_node().filter(|_| !is_parser_dstr(node)).map(|s| s.unescaped().to_vec())
}

fn complex_content(values: &[Node], re: Option<&Regex>) -> bool {
    values.iter().any(|v| {
        let Some(bytes) = str_content(v) else { return false };
        let Ok(s) = std::str::from_utf8(&bytes) else { return true };
        re.is_some_and(|re| !re.is_match(s)) || s.contains(' ')
    })
}

fn word_regex(cx: &Cx) -> Option<Regex> {
    let raw = match cx.cfg.get("WordRegex")? {
        Value::Regex(r) => r.clone(),
        v => v.as_string()?,
    };
    // Rust's `\w` is Unicode `\p{Word}`.
    ruby_regex(&raw.replace(r"\p{Word}", r"\w"))
}

#[derive(Default)]
struct MatrixCache(HashMap<(usize, usize), bool>);

impl Cop for WordArray {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        let open = array.opening_loc().map(|l| text(&l)).unwrap_or("");
        let percent_style = cx.cfg.str("EnforcedStyle") != Some("brackets");
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        let els: Vec<Node> = array.elements().iter().collect();
        if open == "[" {
            if els.is_empty() || !els.iter().all(|e| str_content(e).is_some()) || els.iter().any(is_heredoc) {
                return;
            }
            let re = word_regex(cx);
            if complex_content(&els, re.as_ref()) || self.within_matrix(cx, re.as_ref()) {
                return;
            }
            if pa::allowed_bracket_array(cx, node) || !percent_style {
                return;
            }
            let values: Vec<Vec<u8>> = els.iter().map(|e| str_content(e).unwrap()).collect();
            let replacement = pa::percent_literal_replacement(cx, node, &values, 'w');
            cx.add_fix(start, end, PERCENT_MSG, |c| c.replace(start, end, &replacement));
        } else if open.starts_with("%w") || open.starts_with("%W") {
            let brackets_required = complex_content(&els, None);
            if percent_style && !brackets_required {
                return;
            }
            let words: Vec<String> = els
                .iter()
                .map(|w| match str_content(w) {
                    Some(v) => pa::to_string_literal(&v),
                    None => pa::trim_string_interpolation_escape_character(&pa::to_string_literal(
                        node_text(w).as_bytes(),
                    )),
                })
                .collect();
            let code =
                if words.is_empty() { "[]".to_string() } else { pa::bracketed_with_whitespace(cx.src, &array, &words) };
            let msg = pa::bracketed_message(&code, "words");
            cx.add_fix(start, end, msg, |c| c.replace(start, end, &code));
        }
    }
}

impl WordArray {
    /// within_matrix_of_complex_content?
    fn within_matrix(&self, cx: &mut Cx, re: Option<&Regex>) -> bool {
        let Some(parent) = cx.ancestors.last().and_then(|p| p.as_array_node()) else { return false };
        let key = (parent.location().start_offset(), parent.location().end_offset());
        if let Some(&v) = cx.state::<MatrixCache>().0.get(&key) {
            return v;
        }
        let subs: Vec<Node> = parent.elements().iter().collect();
        let v = subs.iter().all(|s| s.as_array_node().is_some())
            && subs.iter().any(|s| {
                let vals: Vec<Node> = s.as_array_node().unwrap().elements().iter().collect();
                complex_content(&vals, re)
            });
        cx.state::<MatrixCache>().0.insert(key, v);
        v
    }
}
