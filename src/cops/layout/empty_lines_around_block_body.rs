use crate::prelude::*;

pub const NAME: &str = "Layout/EmptyLinesAroundBlockBody";
pub static COP: EmptyLinesAroundBlockBody = EmptyLinesAroundBlockBody;
pub struct EmptyLinesAroundBlockBody;

impl Cop for EmptyLinesAroundBlockBody {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        let loc = node.location();
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("no_empty_lines").to_string();
        // parser (block send args body): the block spans the call; send_node ends before the block.
        let (send_end, block) = match kind::of(node) {
            kind::LAMBDA_NODE => {
                let l = node.as_lambda_node().unwrap();
                let body = helpers::empty_lines_around_body::body_children(l.body());
                let first = Some(src.line_of(l.operator_loc().start_offset()));
                // BlockNode#single_line?: `{`/`do` and `}`/`end` on one line.
                let range = (l.opening_loc().start_offset(), loc.end_offset());
                helpers::empty_lines_around_body::check(cx, "block", &style, range, body.as_deref(), first);
                return;
            }
            kind::CALL_NODE => {
                let c = node.as_call_node().unwrap();
                (send_range(&c).1, c.block())
            }
            kind::SUPER_NODE => {
                let s = node.as_super_node().unwrap();
                let end = [s.rparen_loc().map(|l| l.end_offset()), s.arguments().map(|a| a.location().end_offset())]
                    .into_iter()
                    .flatten()
                    .max()
                    .unwrap_or(s.keyword_loc().end_offset());
                (end, s.block())
            }
            _ => {
                let s = node.as_forwarding_super_node().unwrap();
                (s.location().start_offset() + 5, s.block().map(|b| b.as_node()))
            }
        };
        let Some(block) = block.and_then(|b| b.as_block_node()) else { return };
        let body = helpers::empty_lines_around_body::body_children(block.body());
        let range = (block.location().start_offset(), block.location().end_offset());
        let first = Some(src.line_of(send_end - 1));
        helpers::empty_lines_around_body::check(cx, "block", &style, range, body.as_deref(), first);
    }
}
