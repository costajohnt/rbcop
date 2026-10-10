use crate::prelude::*;
use crate::source::Source;
use helpers::alignment::{begins_its_line, configured_indentation_width};
use helpers::alignment_corrector::{self as ac, Settings};
use ruby_prism::Location;

pub const NAME: &str = "Layout/ClosingParenthesisIndentation";
pub static COP: ClosingParenthesisIndentation = ClosingParenthesisIndentation;
pub struct ClosingParenthesisIndentation;

/// `processed_source.line_indentation(line)`: length of the line's leading whitespace.
fn line_indentation(src: &Source, line: usize) -> usize {
    src.lines.get(line - 1).map_or(0, |l| l.len() - l.trim_start_matches([' ', '\t', '\x0c', '\x0b', '\r']).len())
}

fn def_params<'pr>(p: &ruby_prism::ParametersNode<'pr>) -> Vec<Node<'pr>> {
    let mut v: Vec<Node> = p.requireds().iter().chain(p.optionals().iter()).collect();
    v.extend(crate::ast::parser_rest(p.rest()));
    v.extend(p.posts().iter());
    v.extend(p.keywords().iter());
    v.extend(p.keyword_rest());
    v.extend(p.block().map(|b| b.as_node()));
    v.sort_by_key(|n| n.location().start_offset());
    v
}

impl Cop for ClosingParenthesisIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::PARENTHESES_NODE, kind::EMBEDDED_STATEMENTS_NODE, kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (node_start, left, right, elements): (usize, Location, Location, Vec<Node>) = match kind::of(node) {
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                let (Some(l), Some(r)) = (call.opening_loc(), call.closing_loc()) else { return };
                if l.as_slice() != b"(" {
                    return;
                }
                (node.location().start_offset(), l, r, send_args(&call))
            }
            kind::PARENTHESES_NODE => {
                let p = node.as_parentheses_node().unwrap();
                let elements = match p.body() {
                    None => vec![],
                    Some(b) => match b.as_statements_node() {
                        Some(s) => s.body().iter().collect(),
                        None => vec![b],
                    },
                };
                (node.location().start_offset(), p.opening_loc(), p.closing_loc(), elements)
            }
            // `#{...}` is a parser `begin` too (`on_begin` takes its `}` for the right paren).
            kind::EMBEDDED_STATEMENTS_NODE => {
                let e = node.as_embedded_statements_node().unwrap();
                let elements = e.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
                (node.location().start_offset(), e.opening_loc(), e.closing_loc(), elements)
            }
            _ => {
                let d = node.as_def_node().unwrap();
                let (Some(l), Some(r)) = (d.lparen_loc(), d.rparen_loc()) else { return };
                (l.start_offset(), l, r, d.parameters().map(|p| def_params(&p)).unwrap_or_default())
            }
        };
        let src = cx.src;
        let rp = right.start_offset();
        if !begins_its_line(src, rp) {
            return;
        }
        let line = |off: usize| src.line_of(off);
        let col = |off: usize| src.column(off) as i64;
        let left_col = col(left.start_offset());
        let right_col = col(rp);
        let correct = if let Some(first) = elements.first() {
            let first_line = line(first.location().start_offset());
            if first_line > line(left.start_offset()) {
                (line_indentation(src, first_line) as i64 - configured_indentation_width(cx)).max(0)
            } else {
                let cols: Vec<i64> = match first.as_keyword_hash_node().map(|h| h.elements()).or_else(|| first.as_hash_node().map(|h| h.elements())) {
                    Some(children) => children.iter().map(|c| col(c.location().start_offset())).collect(),
                    None => elements.iter().map(|e| col(e.location().start_offset())).collect(),
                };
                if !cols.is_empty() && cols.iter().all(|&c| c == cols[0]) {
                    left_col
                } else {
                    line_indentation(src, first_line) as i64
                }
            }
        } else {
            let candidates =
                [line_indentation(src, line(left.start_offset())) as i64, left_col, col(node_start)];
            if candidates.contains(&right_col) {
                return;
            }
            candidates[0]
        };
        let delta = correct - right_col;
        if delta == 0 {
            return;
        }
        let msg = if correct == left_col {
            "Align `)` with `(`.".to_string()
        } else {
            format!("Indent `)` to column {correct} (not {right_col})")
        };
        let (re, set) = (right.end_offset(), Settings::of(cx));
        cx.add_fix(rp, re, msg, |c| ac::correct(c, src, set, rp, re, None, delta, false));
    }
}
