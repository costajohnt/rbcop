use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/RedundantSort";
pub static COP: RedundantSort = RedundantSort;
pub struct RedundantSort;

fn has_block(call: &CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_node().is_some())
}

/// Parser `send` arguments: positional args plus a `&blk` block pass.
fn parser_args<'pr>(call: &CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut args = call_args(call);
    if let Some(b) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
        args.push(b);
    }
    args
}

impl Cop for RedundantSort {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let sort = node.as_call_node().unwrap();
        let sorter = method_name(&sort);
        if sorter != "sort" && sorter != "sort_by" {
            return;
        }
        // The call this sort is the receiver of (directly, or through its block).
        let n = cx.ancestors.len();
        let Some(outer) = cx.ancestors.last().and_then(|a| a.as_call_node()) else { return };
        let loc = node.location();
        if outer.receiver().is_none_or(|r| r.location().start_offset() != loc.start_offset() || r.location().end_offset() != loc.end_offset()) {
            return;
        }
        let accessor = method_name(&outer);
        let sort_args = parser_args(&sort);
        let outer_args = parser_args(&outer);
        let index_accessor = matches!(accessor, "[]" | "at" | "slice");
        let arg = if index_accessor {
            match outer_args.as_slice() {
                [a] => match int_literal_value(a) {
                    Some(v @ (0 | -1)) => Some(v),
                    _ => return,
                },
                _ => return,
            }
        } else if matches!(accessor, "first" | "last") && outer_args.is_empty() {
            None
        } else {
            return;
        };
        let ok = if has_block(&sort) {
            sort_args.is_empty()
        } else if sorter == "sort" {
            sort_args.is_empty()
        } else {
            // sort_by with one argument; the [] form only for plain sends.
            sort_args.len() == 1 && (!index_accessor || (!is_csend(&outer) && !is_csend(&sort)))
        };
        if !ok {
            return;
        }
        let Some(sort_sel) = sort.message_loc() else { return };
        let Some(sel) = outer.message_loc() else { return };
        let end = send_range(&outer).1;
        let base = if accessor == "first" || arg == Some(0) { "min" } else { "max" };
        let suggestion = format!("{base}{}", if sorter == "sort_by" { "_by" } else { "" });
        let accessor_source = &cx.src.text[sel.start_offset()..end];
        let msg = format!("Use `{suggestion}` instead of `{sorter}...{accessor_source}`.");
        let accessor_start = outer.call_operator_loc().map_or(sel.start_offset(), |d| d.start_offset());
        // `with_logical_operator?`: the parser parent of the accessor send is `and`/`or`.
        let op = if has_block(&outer) {
            None
        } else {
            parser_parent(&cx.ancestors[..n - 1]).and_then(|p| {
                p.as_and_node().map(|a| a.operator_loc()).or_else(|| p.as_or_node().map(|o| o.operator_loc()))
            })
        };
        let op = op.map(|l| (l.start_offset(), l.end_offset(), text(&l).to_string()));
        let recv_range = (loc.start_offset(), loc.end_offset());
        cx.add_fix(sort_sel.start_offset(), end, msg, |c| {
            c.remove(accessor_start, end);
            c.replace(sort_sel.start_offset(), sort_sel.end_offset(), &suggestion);
            if let Some((s, e, op)) = &op {
                c.insert_after(recv_range.0, recv_range.1, &format!(" {op}"));
                c.remove(*s, *e);
            }
        });
    }
}

/// The parser-gem parent of the node whose Prism ancestors are given.
fn parser_parent<'a, 'pr>(ancestors: &'a [Node<'pr>]) -> Option<&'a Node<'pr>> {
    (0..ancestors.len()).rev().find_map(|i| {
        let f = frame(&ancestors[i]);
        let owns_block = f == Frame::Call && ancestors.get(i + 1).is_some_and(|n| frame(n) == Frame::Block);
        (f != Frame::Transparent && !owns_block).then(|| &ancestors[i])
    })
}
