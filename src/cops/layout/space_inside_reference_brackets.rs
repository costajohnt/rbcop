use crate::prelude::*;
use helpers::surrounding_space as ss;

pub const NAME: &str = "Layout/SpaceInsideReferenceBrackets";
pub static COP: SpaceInsideReferenceBrackets = SpaceInsideReferenceBrackets;
pub struct SpaceInsideReferenceBrackets;

/// Sorted parser-like tokens, computed once per file.
#[derive(Default)]
struct State(Option<Vec<Token>>);

fn left_ref(t: &Token) -> bool {
    t.kind == sys::PM_TOKEN_BRACKET_LEFT
}

fn left_bracket(t: &Token) -> bool {
    matches!(t.kind, sys::PM_TOKEN_BRACKET_LEFT | sys::PM_TOKEN_BRACKET_LEFT_ARRAY)
}

fn right_bracket(t: &Token) -> bool {
    t.kind == sys::PM_TOKEN_BRACKET_RIGHT
}

impl Cop for SpaceInsideReferenceBrackets {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::INDEX_OPERATOR_WRITE_NODE,
            kind::INDEX_OR_WRITE_NODE,
            kind::INDEX_AND_WRITE_NODE,
            kind::INDEX_TARGET_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // The parser `send` (:[] / :[]=) this node stands for: its range and whether it is `[]=`.
        let (start, end, setter) = match kind::of(node) {
            kind::CALL_NODE => {
                let call = node.as_call_node().unwrap();
                let name = method_name(&call);
                if is_csend(&call) || !matches!(name, "[]" | "[]=") {
                    return;
                }
                let (s, e) = send_range(&call);
                (s, e, name == "[]=")
            }
            kind::INDEX_TARGET_NODE => {
                let l = node.location();
                (l.start_offset(), l.end_offset(), true)
            }
            k => {
                let (recv, close) = match k {
                    kind::INDEX_OPERATOR_WRITE_NODE => {
                        let n = node.as_index_operator_write_node().unwrap();
                        (n.receiver(), n.closing_loc())
                    }
                    kind::INDEX_OR_WRITE_NODE => {
                        let n = node.as_index_or_write_node().unwrap();
                        (n.receiver(), n.closing_loc())
                    }
                    _ => {
                        let n = node.as_index_and_write_node().unwrap();
                        (n.receiver(), n.closing_loc())
                    }
                };
                let Some(recv) = recv else { return };
                (recv.location().start_offset(), close.end_offset(), false)
            }
        };
        let src = cx.src;
        if cx.state::<State>().0.is_none() {
            let mut t = helpers::parser_tokens::sorted_tokens(src);
            t.sort_by_key(|t| t.start);
            cx.state::<State>().0 = Some(t);
        }
        let all = cx.state::<State>().0.as_ref().unwrap();
        // tokens_within
        let first = all.partition_point(|t| t.start < start);
        let last = all.partition_point(|t| t.end < end);
        if first >= all.len() || last < first {
            return;
        }
        let tokens: Vec<Token> = all[first..=last.min(all.len() - 1)].to_vec();
        // left_ref_bracket
        let Some(current) = tokens.iter().rposition(left_ref) else { return };
        let previous = (first + current).checked_sub(1).map(|i| all[i]);
        let left_i = if setter || previous.is_some_and(|p| !right_bracket(&p)) {
            tokens.iter().position(left_ref).unwrap()
        } else {
            current
        };
        let left = tokens[left_i];
        // closing_bracket
        let mut depth = 0i32;
        let mut right_i = None;
        for (i, t) in tokens.iter().enumerate().skip(left_i) {
            if left_bracket(t) {
                depth += 1;
            }
            if right_bracket(t) {
                depth -= 1;
                if depth == 0 {
                    right_i = Some(i);
                    break;
                }
            }
        }
        let empty_config = cx.cfg.str("EnforcedStyleForEmptyBrackets").unwrap_or("no_space").to_string();
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("no_space").to_string();
        let right = right_i.map(|i| tokens[i]);
        if let (Some(r), Some(ri)) = (right, right_i)
            && left_i + 1 == ri
        {
            let Some(command) = ss::empty_offense_command(src, &empty_config, &left, &r) else { return };
            let msg = format!("{command} space inside empty reference brackets.");
            cx.add_fix(left.start, r.end, msg, |c| ss::empty_corrections(src, c, &empty_config, &left, &r));
            return;
        }
        if src.line_of(start) != src.line_of(end) {
            return;
        }
        let no_space = style == "no_space";
        let planned = if no_space {
            ss::no_space_offenses(src, Some(&left), right.as_ref(), false, false, ss::uncorrectable(cx))
        } else {
            ss::space_offenses(src, Some(&left), right.as_ref(), false, false, ss::uncorrectable(cx))
        };
        let correct = |c: &mut Corrector| {
            let Some(r) = right else { return };
            if no_space {
                ss::remove_space(src, c, &left, &r);
            } else {
                ss::add_space(src, c, &left, &r);
            }
        };
        let mut ignored = Vec::new();
        for p in planned {
            ss::space_offense(cx, &mut ignored, (start, end), p, "%<command>s space inside reference brackets.", &correct);
        }
    }
}
