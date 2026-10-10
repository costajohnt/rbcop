use crate::prelude::*;

pub const NAME: &str = "Lint/SuppressedException";
pub static COP: SuppressedException = SuppressedException;
pub struct SuppressedException;

/// Last line (1-based) of the nearest kwbegin / def / block ancestor: its `end`, else its last line.
fn enclosing_end_line(cx: &Cx) -> Option<usize> {
    let end = cx.ancestors.iter().rev().find_map(|a| match kind::of(a) {
        kind::BEGIN_NODE => {
            let b = a.as_begin_node().unwrap();
            b.begin_keyword_loc()?;
            Some(b.end_keyword_loc().map_or(a.location().end_offset() - 1, |l| l.start_offset()))
        }
        kind::DEF_NODE => {
            let d = a.as_def_node().unwrap();
            Some(d.end_keyword_loc().map_or(a.location().end_offset() - 1, |l| l.start_offset()))
        }
        kind::BLOCK_NODE => Some(a.as_block_node().unwrap().closing_loc().start_offset()),
        kind::LAMBDA_NODE => Some(a.as_lambda_node().unwrap().closing_loc().start_offset()),
        _ => None,
    })?;
    Some(cx.src.line_of(end))
}

impl Cop for SuppressedException {
    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_NODE, kind::RESCUE_MODIFIER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (body, (s, e)) = match node.as_rescue_node() {
            Some(r) => {
                let body = r.statements().map(|s| s.body().iter().collect::<Vec<_>>());
                (body, helpers::resbody::range(cx.src, &r))
            }
            None => {
                let m = node.as_rescue_modifier_node().unwrap();
                (Some(vec![m.rescue_expression()]), helpers::resbody::modifier_range(&m))
            }
        };
        let nil_body = body.as_ref().is_some_and(|b| b.len() == 1 && b[0].as_nil_node().is_some());
        if body.is_some() && !nil_body {
            return;
        }
        if cx.cfg.bool("AllowComments") == Some(true)
            && let Some(end_line) = enclosing_end_line(cx)
        {
            let first = cx.src.line_of(s);
            // processed_source[first_line...end_line]: 0-based indexes, so the lines after the rescue line.
            let lines = cx.src.lines.get(first..end_line.min(cx.src.lines.len())).unwrap_or(&[]);
            if lines.iter().any(|l| l.trim_start_matches(|c: char| c.is_ascii_whitespace()).starts_with('#')) {
                return;
            }
        }
        if cx.cfg.bool("AllowNil") == Some(true) && nil_body {
            return;
        }
        cx.add(s, e, "Do not suppress exceptions.");
    }
}
