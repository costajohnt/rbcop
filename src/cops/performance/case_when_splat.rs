use crate::prelude::*;

pub const NAME: &str = "Performance/CaseWhenSplat";
pub static COP: CaseWhenSplat = CaseWhenSplat;
pub struct CaseWhenSplat;

/// non_splat?: not a splat, or a splat of an array literal.
fn non_splat(condition: &Node) -> bool {
    condition.as_splat_node().is_none_or(|s| s.expression().is_some_and(|e| e.as_array_node().is_some()))
}

impl Cop for CaseWhenSplat {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_node().unwrap();
        let whens: Vec<ruby_prism::WhenNode> = case.conditions().iter().filter_map(|w| w.as_when_node()).collect();
        // (when index, condition index) of every condition, in source order.
        let all: Vec<(usize, usize)> = whens
            .iter()
            .enumerate()
            .flat_map(|(w, when)| (0..when.conditions().iter().count()).map(move |c| (w, c)))
            .collect();
        let condition = |(w, c): (usize, usize)| whens[w].conditions().iter().nth(c).unwrap();
        // splat_offenses: splats with a non-splat condition somewhere after them.
        let mut found_non_splat = false;
        let mut offenses = Vec::new();
        for &pos in all.iter().rev() {
            let cond = condition(pos);
            found_non_splat |= non_splat(&cond);
            if !non_splat(&cond) && found_non_splat {
                offenses.push(pos);
            }
        }
        let src = cx.src;
        let mut ignored: Vec<usize> = Vec::new();
        for &(w, c) in offenses.iter().rev() {
            if ignored.contains(&w) {
                continue;
            }
            ignored.push(w);
            let when = &whens[w];
            let cond = condition((w, c));
            let start = when.keyword_loc().start_offset();
            let end = cond.location().end_offset();
            let msg = "Reordering `when` conditions with a splat to the end of the `when` branches can improve performance.";
            let conditions: Vec<Node> = when.conditions().iter().collect();
            // replacement: non-splats first, then splats.
            let (splats, others): (Vec<&Node>, Vec<&Node>) = conditions.iter().partition(|n| n.as_splat_node().is_some());
            let replacement = others.iter().chain(&splats).map(|n| node_text(n)).collect::<Vec<_>>().join(", ");
            // needs_reorder?: a later branch has a non-splat condition.
            let needs_reorder = whens[w + 1..].iter().any(|b| b.conditions().iter().any(|c| non_splat(&c)));
            let edit: Option<Vec<(usize, usize, String)>> = if !needs_reorder {
                let first = conditions.first().unwrap().location().start_offset();
                let last = conditions.last().unwrap().location().end_offset();
                Some(vec![(first, last, replacement)])
            } else if whens.len() == 1 {
                None
            } else {
                let when_start = when.location().start_offset();
                let next_start = whens[w + 1].location().start_offset();
                let last_end = when_end(whens.last().unwrap());
                let body = when.statements();
                let indent = " ".repeat(src.column(when_start));
                let body_line = body.as_ref().map(|b| src.line_of(b.location().start_offset()));
                let mut text = if body_line == Some(src.line_of(when_start)) {
                    let b = body.as_ref().unwrap().location();
                    format!("\n{indent}when {replacement} then {}", src.slice(b.start_offset(), b.end_offset()))
                } else {
                    let mut t = format!("\n{indent}when {replacement}\n");
                    if let Some(b) = &body {
                        let l = b.location();
                        t.push_str(&" ".repeat(src.column(l.start_offset())));
                        t.push_str(src.slice(l.start_offset(), l.end_offset()));
                    }
                    t
                };
                // comments_in_range(when_node): its first line up to find_end_line.
                let end_line = match whens.get(w + 1) {
                    Some(next) => src.line_of(next.location().start_offset()),
                    None => unreachable!(),
                };
                let comments: Vec<String> = src
                    .comments
                    .iter()
                    .filter(|cm| (src.line_of(when_start)..end_line).contains(&src.line_of(cm.start)))
                    .map(|cm| format!("{}{}", " ".repeat(src.column(cm.start)), src.slice(cm.start, cm.end)))
                    .collect();
                text.push_str(&comments.join("\n"));
                Some(vec![(when_start, next_start, String::new()), (last_end, last_end, text)])
            };
            let last_when_start = whens.last().unwrap().location().start_offset();
            cx.add_fix(start, end, msg, |c| {
                let Some(edits) = &edit else { return };
                for (i, (s, e, t)) in edits.iter().enumerate() {
                    if needs_reorder && i == 1 {
                        c.insert_after(last_when_start, *e, t);
                    } else if t.is_empty() {
                        c.remove(*s, *e);
                    } else {
                        c.replace(*s, *e, t);
                    }
                }
            });
        }
    }
}

/// The parser `when` node's end: its body's end, else its last condition's.
fn when_end(when: &ruby_prism::WhenNode) -> usize {
    match when.statements() {
        Some(s) => s.location().end_offset(),
        None => when.conditions().iter().last().map_or(when.location().end_offset(), |c| c.location().end_offset()),
    }
}
