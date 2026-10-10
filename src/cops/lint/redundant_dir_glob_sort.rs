use crate::prelude::*;

pub const NAME: &str = "Lint/RedundantDirGlobSort";
pub static COP: RedundantDirGlobSort = RedundantDirGlobSort;
pub struct RedundantDirGlobSort;

impl Cop for RedundantDirGlobSort {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.0, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "sort" || is_csend(&call) {
            return;
        }
        // ponytail: a receiver with a literal block (`Dir.glob(x) { }.sort`) is skipped.
        let Some(glob) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if glob.block().is_some_and(|b| b.as_block_node().is_some()) {
            return;
        }
        let Some(dir) = glob.receiver() else { return };
        let short = match (dir.as_constant_read_node(), dir.as_constant_path_node()) {
            (Some(c), _) => Some(c.name()),
            (_, Some(p)) => p.name(),
            _ => return,
        };
        if short.is_none_or(|n| n.as_slice() != b"Dir") || !matches!(method_name(&glob), "glob" | "[]") {
            return;
        }
        // sort_with_comparator?: `node.parent&.any_block_type?` is also true when the call is a
        // block's sole body statement.
        let parent_block = helpers::parser_parent::parser_parent(cx.ancestors).is_some_and(|p| frame(p) == Frame::Block);
        if call.block().is_some() || parent_block {
            return;
        }
        let args = send_args(&glob);
        if args.len() >= 2 || args.first().is_some_and(|a| a.as_splat_node().is_some()) {
            return;
        }
        let (Some(sel), Some(dot)) = (call.message_loc(), call.call_operator_loc()) else { return };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        let (ds, de) = (dot.start_offset(), dot.end_offset());
        cx.add_fix(s, e, "Remove redundant `sort`.", |c| {
            c.remove(s, e);
            c.remove(ds, de);
        });
    }
}
