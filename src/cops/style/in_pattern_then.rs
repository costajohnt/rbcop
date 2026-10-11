use crate::prelude::*;

pub const NAME: &str = "Style/InPatternThen";
pub static COP: InPatternThen = InPatternThen;
pub struct InPatternThen;

/// `collect_alternative_patterns`: the alternatives of a left-nested `a | b | c`.
fn alternatives(node: &Node, out: &mut Vec<String>) {
    let alt = node.as_alternation_pattern_node().unwrap();
    let left = alt.left();
    if left.as_alternation_pattern_node().is_some() {
        alternatives(&left, out);
    } else {
        out.push(node_text(&left).to_string());
    }
    out.push(node_text(&alt.right()).to_string());
}

impl Cop for InPatternThen {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let in_node = node.as_in_node().unwrap();
        if in_node.then_loc().is_some() || in_node.statements().is_none() {
            return;
        }
        let loc = node.location();
        if cx.src.line_of(loc.start_offset()) != cx.src.line_of(loc.end_offset()) {
            return;
        }
        // The guard (`in pat if cond`) wraps the pattern in Prism; parser keeps them apart.
        let mut pattern = in_node.pattern();
        let head_end = pattern.location().end_offset();
        if let Some(i) = pattern.as_if_node() {
            pattern = i.statements().and_then(|s| s.body().iter().next()).unwrap_or(pattern);
        } else if let Some(u) = pattern.as_unless_node() {
            pattern = u.statements().and_then(|s| s.body().iter().next()).unwrap_or(pattern);
        }
        // `loc.begin`: the `;` after the pattern (and guard).
        let bytes = cx.src.text.as_bytes();
        let mut semi = head_end;
        // Any whitespace the lexer skips (`\r`, `\v`, `\f` included; a newline makes the node multi-line).
        while semi < bytes.len() && helpers::parser_tokens::is_ruby_space(bytes[semi]) {
            semi += 1;
        }
        if bytes.get(semi) != Some(&b';') {
            return;
        }
        let source = if pattern.as_alternation_pattern_node().is_some() {
            let mut out = Vec::new();
            alternatives(&pattern, &mut out);
            out.join(" | ")
        } else {
            node_text(&pattern).to_string()
        };
        let msg = format!("Do not use `in {source};`. Use `in {source} then` instead.");
        cx.add_fix(semi, semi + 1, msg, |c| c.replace(semi, semi + 1, " then"));
    }
}
