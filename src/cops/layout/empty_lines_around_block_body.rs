use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundBlockBody";
pub static COP: EmptyLinesAroundBlockBody = EmptyLinesAroundBlockBody;
pub struct EmptyLinesAroundBlockBody;

/// body_start_line: the `{`/`do` line, or the last line of non-empty block arguments if later.
fn body_start_line(cx: &Cx, opening: usize, params: Option<Node>) -> usize {
    let begin = cx.src.line_of(opening);
    // `arguments?`: numbered/`it` parameters and an empty `||` are no arguments.
    let args_end = params.and_then(|p| {
        let b = p.as_block_parameters_node()?;
        let any = b.parameters().is_some() || b.locals().iter().next().is_some();
        any.then(|| p.location().end_offset())
    });
    args_end.map_or(begin, |e| begin.max(cx.src.line_of(e - 1)))
}

impl Cop for EmptyLinesAroundBlockBody {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("no_empty_lines").to_string();
        // parser (block send args body): the block spans the call.
        let block = match kind::of(node) {
            kind::LAMBDA_NODE => {
                let l = node.as_lambda_node().unwrap();
                let body = helpers::empty_lines_around_body::body_children(l.body());
                let first = Some(body_start_line(cx, l.opening_loc().start_offset(), l.parameters()));
                // BlockNode#single_line?: `{`/`do` and `}`/`end` on one line.
                let range = (l.opening_loc().start_offset(), loc.end_offset());
                helpers::empty_lines_around_body::check(cx, "block", &style, range, body.as_deref(), first);
                return;
            }
            kind::CALL_NODE => node.as_call_node().unwrap().block(),
            kind::SUPER_NODE => node.as_super_node().unwrap().block(),
            _ => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        };
        let Some(block) = block.and_then(|b| b.as_block_node()) else { return };
        let body = helpers::empty_lines_around_body::body_children(block.body());
        let range = (block.location().start_offset(), block.location().end_offset());
        let first = Some(body_start_line(cx, block.opening_loc().start_offset(), block.parameters()));
        helpers::empty_lines_around_body::check(cx, "block", &style, range, body.as_deref(), first);
    }
}
