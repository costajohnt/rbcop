use crate::helpers::block_args::{Single, single_body, single_var};
use crate::helpers::select_by::{is_block_var, negation_receiver, paren_single, plain_send, receiver_allowed};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/SelectByRegexp";
pub static COP: SelectByRegexp = SelectByRegexp;
pub struct SelectByRegexp;

impl Cop for SelectByRegexp {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        let select = matches!(method, "select" | "filter" | "find_all");
        if !select && method != "reject" {
            return;
        }
        let target = cx.config.target_ruby;
        if target < 2.6 && method == "filter" {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if receiver_allowed(call.receiver()) {
            return;
        }
        let Some((single, var)) = single_var(&block) else { return };
        let Some(body) = single_body(&block) else { return };
        // `/re/ =~ x` with named captures (parser match_with_lvasgn) behaves like the plain send here.
        let (bang, inner) = match negation_receiver(&body) {
            Some(r) => (true, unwrap_match_write(paren_single(&r).unwrap_or(r))),
            None => (false, unwrap_match_write(body)),
        };
        let Some(check) = plain_send(&inner) else { return };
        let name = method_name(&check);
        let methods_ok = match name {
            "match?" | "=~" => true,
            "!~" => !bang,
            _ => false,
        };
        if !methods_ok || send_args(&check).len() != 1 {
            return;
        }
        // calls_lvar?: the receiver or the last argument is the block variable.
        let args = send_args(&check);
        let recv = check.receiver();
        if !recv.as_ref().is_some_and(|r| is_block_var(r, single, var)) && !is_block_var(&args[0], single, var) {
            return;
        }
        if name == "match?" && recv.is_none() {
            return;
        }
        let negated = bang || name == "!~";
        let replacement = if select == negated { "grep_v" } else { "grep" };
        if target <= 2.2 && replacement == "grep_v" {
            return;
        }
        let regexp = find_regexp(&check, single, var);
        let msg = format!("Prefer `{replacement}` to `{method}` with a regexp match.");
        let Some(sel) = call.message_loc() else { return };
        let (start, end) = (sel.start_offset(), block.closing_loc().end_offset());
        let fix = regexp.map(|r| format!("{replacement}({r})"));
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| {
            if let Some(fix) = &fix {
                c.replace(start, end, fix);
            }
        });
    }
}

fn unwrap_match_write(node: Node) -> Node {
    match node.as_match_write_node() {
        Some(m) => m.call().as_node(),
        None => node,
    }
}

fn is_lvar(node: &Node) -> bool {
    node.as_local_variable_read_node().is_some() || node.as_it_local_variable_read_node().is_some()
}

/// `find_regexp`: the side of the match that is not the block variable.
fn find_regexp<'pr>(check: &CallNode<'pr>, single: Single, var: &[u8]) -> Option<&'pr str> {
    let recv = check.receiver()?;
    let arg = send_args(check).into_iter().next()?;
    if is_lvar(&recv) && (single != Single::Arg || node_text(&recv).as_bytes() == var) {
        Some(node_text(&arg))
    } else if is_lvar(&arg) {
        Some(node_text(&recv))
    } else {
        None
    }
}
