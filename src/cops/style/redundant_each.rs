use crate::prelude::*;
use helpers::parser_parent::parser_parent;

pub const NAME: &str = "Style/RedundantEach";
pub static COP: RedundantEach = RedundantEach;
pub struct RedundantEach;

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn has_block_node(call: &ruby_prism::CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_node().is_some())
}

fn ends_with_block_pass(call: &ruby_prism::CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_argument_node().is_some())
}

const RESTRICT: &[&str] = &["each", "each_with_index", "each_with_object"];

impl Cop for RedundantEach {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !RESTRICT.contains(&name) || ends_with_block_pass(&call) {
            return;
        }
        let src = cx.src;
        let anc = cx.ancestors;
        let parent = parser_parent(anc);
        let node_range = send_range(&call);
        // (selector, method name) of the redundant node
        let mut redundant: Option<((usize, usize), &str)> = None;
        if name == "each" {
            let parent_is_block = has_block_node(&call)
                || parent.is_some_and(|p| matches!(kind::of(p), kind::BLOCK_NODE | kind::LAMBDA_NODE));
            if !parent_is_block {
                // each_ancestor(:call).detect { |a| a.receiver == node && ... }
                redundant = (0..anc.len()).rev().find_map(|i| {
                    let a = anc[i].as_call_node()?;
                    if anc.get(i + 1).is_some_and(|n| kind::of(n) == kind::BLOCK_NODE) {
                        return None;
                    }
                    let m = method_name(&a);
                    if !(RESTRICT.contains(&m) || m == "reverse_each") {
                        return None;
                    }
                    let r = a.receiver()?;
                    if !helpers::node_equality::same(src, range(&r), node_range) {
                        return None;
                    }
                    let sel = a.message_loc()?;
                    Some(((sel.start_offset(), sel.end_offset()), m))
                });
            }
        }
        if redundant.is_none() {
            let Some(prev) = call.receiver().and_then(|r| r.as_call_node()) else { return };
            if has_block_node(&prev) || ends_with_block_pass(&prev) {
                return;
            }
            let m = method_name(&prev);
            let detected = name != "each" && m.starts_with("each_");
            if !(detected || m == "reverse_each") {
                return;
            }
            let Some(sel) = prev.message_loc() else { return };
            redundant = Some(((sel.start_offset(), sel.end_offset()), m));
        }
        let Some((red_sel, red_name)) = redundant else { return };
        let Some(sel) = call.message_loc() else { return };
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (start, end) = if name != "each" {
            (ss, se)
        } else if let Some(p) = parent.and_then(|p| p.as_call_node()).filter(|_| !has_block_node(&call)) {
            let Some(d) = p.call_operator_loc() else { return };
            (ss.min(d.start_offset()), se.max(d.end_offset()))
        } else {
            let Some(d) = call.call_operator_loc() else { return };
            (d.start_offset().min(ss), se.max(d.end_offset()))
        };
        let msg = match name {
            "each" => "Remove redundant `each`.",
            "each_with_index" => "Use `with_index` to remove redundant `each`.",
            _ => "Use `with_object` to remove redundant `each`.",
        };
        cx.add_fix(start, end, msg, |c| match name {
            "each" => {
                c.remove(start, end);
                match red_name {
                    "each_with_index" => c.replace(red_sel.0, red_sel.1, "each.with_index"),
                    "each_with_object" => c.replace(red_sel.0, red_sel.1, "each.with_object"),
                    _ => {}
                }
            }
            "each_with_index" => c.replace(ss, se, "with_index"),
            _ => c.replace(ss, se, "with_object"),
        });
    }
}
