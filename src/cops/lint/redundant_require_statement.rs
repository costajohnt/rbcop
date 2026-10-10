use crate::prelude::*;

pub const NAME: &str = "Lint/RedundantRequireStatement";
pub static COP: RedundantRequireStatement = RedundantRequireStatement;
pub struct RedundantRequireStatement;

fn redundant_feature(name: &[u8], target: f64) -> bool {
    name == b"enumerator"
        || (target >= 2.1 && name == b"thread")
        || (target >= 2.2 && (name == b"rational" || name == b"complex"))
        || (target >= 2.7 && name == b"ruby2_keywords")
        || (target >= 3.1 && name == b"fiber")
        || (target >= 3.2 && name == b"set")
        || (target >= 4.0 && name == b"pathname")
}

/// Range of the parser parent when it is a modifier `if`/`unless`/`while`/`until`.
fn modifier_parent(ancestors: &[Node]) -> Option<(usize, usize)> {
    let parent = ancestors.iter().rev().find(|a| frame(a) != Frame::Transparent)?;
    let modifier = if let Some(i) = parent.as_if_node() {
        i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && i.end_keyword_loc().is_none()
    } else if let Some(u) = parent.as_unless_node() {
        u.end_keyword_loc().is_none()
    } else if let Some(w) = parent.as_while_node() {
        w.closing_loc().is_none() && !w.is_begin_modifier()
    } else if let Some(u) = parent.as_until_node() {
        u.closing_loc().is_none() && !u.is_begin_modifier()
    } else {
        false
    };
    let loc = parent.location();
    modifier.then(|| (loc.start_offset(), loc.end_offset()))
}

impl Cop for RedundantRequireStatement {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :require (str #redundant_feature?))
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "require" {
            return;
        }
        let args = send_args(&call);
        if args.len() != 1 {
            return;
        }
        let Some(s) = args[0].as_string_node() else { return };
        if !redundant_feature(s.unescaped(), cx.config.target_ruby) {
            return;
        }
        let (start, end) = send_range(&call);
        let src = cx.src;
        let modifier = modifier_parent(cx.ancestors);
        let range = match modifier {
            Some(_) => {
                use helpers::range_help::{range_with_surrounding_space, Side, Surrounding};
                range_with_surrounding_space(src.text, start, end, Surrounding { side: Side::Right, ..Default::default() })
            }
            None => {
                // range_by_whole_lines(include_final_newline: true), clipped to the buffer
                let first = src.line_start(src.line_of(start));
                let last_line = src.line_of(end);
                let line_end = src.line_start(last_line) + src.text[src.line_start(last_line)..].find('\n').unwrap_or(src.text.len() - src.line_start(last_line));
                (first, (line_end + 1).min(src.text.len()))
            }
        };
        cx.add_fix(start, end, "Remove unnecessary `require` statement.", |c| {
            if let Some((ps, pe)) = modifier {
                c.insert_after(ps, pe, "\nend");
            }
            c.remove(range.0, range.1);
        });
    }
}
