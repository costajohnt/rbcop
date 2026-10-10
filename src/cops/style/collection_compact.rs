use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/CollectionCompact";
pub static COP: CollectionCompact = CollectionCompact;
pub struct CollectionCompact;

/// `(call (lvar name) :nil?)` (send or csend, no arguments): the lvar's name.
fn nil_check<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = node.as_call_node()?;
    if method_name(&call) != "nil?" || call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    call.receiver()
        .filter(|r| r.as_local_variable_read_node().is_some() || r.as_it_local_variable_read_node().is_some())
}

/// `(call (call $(lvar _) :nil?) :!)`
fn not_nil_check<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = node.as_call_node()?;
    if method_name(&call) != "!" || call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    nil_check(&call.receiver()?)
}

/// `match_block_method?` for the block attached to `call`.
fn match_block(call: &CallNode, select: bool) -> bool {
    let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return false };
    if call.receiver().is_none() || call.arguments().is_some() {
        return false;
    }
    let Some(stmts) = block.body().and_then(|b| b.as_statements_node()) else { return false };
    let mut it = stmts.body().iter();
    let (Some(body), None) = (it.next(), it.next()) else { return false };
    let Some(lvar) = (if select { not_nil_check(&body) } else { nil_check(&body) }) else { return false };
    let lvar_name = |n: &Node| n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == b"_1");
    match block.parameters() {
        Some(p) if p.as_numbered_parameters_node().is_some() => lvar_name(&lvar),
        Some(p) if p.as_it_parameters_node().is_some() => lvar.as_it_local_variable_read_node().is_some(),
        params => {
            if lvar.as_local_variable_read_node().is_none() {
                return false;
            }
            // `args.last.source == receiver.source`; no parameters: `nil.source` raises.
            let Some(bp) = params.and_then(|p| p.as_block_parameters_node()) else { return false };
            let mut nodes = bp.parameters().map(|ps| parameter_nodes(&ps)).unwrap_or_default();
            nodes.extend(bp.locals().iter());
            nodes.last().is_some_and(|last| node_text(last) == node_text(&lvar))
        }
    }
}

impl Cop for CollectionCompact {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (2.4, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "reject" | "reject!" | "select" | "select!" | "filter" | "filter!" | "grep_v") {
            return;
        }
        let ruby = cx.config.target_ruby;
        if ruby < 2.6 && matches!(name, "filter" | "filter!") {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let block_pass_nil = matches!(name, "reject" | "reject!")
            && call.receiver().is_some()
            && call.arguments().is_none()
            && call
                .block()
                .and_then(|b| b.as_block_argument_node())
                .and_then(|b| b.expression())
                .and_then(|e| e.as_symbol_node())
                .is_some_and(|s| s.unescaped() == b"nil?");
        let grep_v_nil = name == "grep_v"
            && call.block().is_none_or(|b| b.as_block_node().is_some())
            && matches!(call_args(&call).as_slice(), [a] if a.as_nil_node().is_some() || is_top_const(a, "NilClass"));
        let end = if block_pass_nil || grep_v_nil {
            send_range(&call).1
        } else if match_block(&call, !matches!(name, "reject" | "reject!")) {
            node.location().end_offset()
        } else {
            return;
        };
        // `allowed_receiver?(nil)` raises.
        let Some(recv) = call.receiver() else { return };
        if helpers::allowed_receivers::allowed_receiver(cx, &recv) {
            return;
        }
        if ruby <= 3.0 && recv.as_call_node().is_some_and(|r| matches!(method_name(&r), "to_enum" | "lazy")) {
            return;
        }
        let good = if name.ends_with('!') { "compact!" } else { "compact" };
        let s = sel.start_offset();
        let msg = format!("Use `{good}` instead of `{}`.", cx.src.slice(s, end));
        cx.add_fix(s, end, msg, |c| c.replace(s, end, good));
    }
}
