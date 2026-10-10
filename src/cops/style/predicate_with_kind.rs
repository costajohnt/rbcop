use crate::prelude::*;

pub const NAME: &str = "Style/PredicateWithKind";
pub static COP: PredicateWithKind = PredicateWithKind;
pub struct PredicateWithKind;

/// The single block parameter's name as the body refers to it: `|x|`, `_1`, `it`.
fn block_arg_name<'pr>(block: &ruby_prism::BlockNode<'pr>) -> Option<&'pr [u8]> {
    let params = block.parameters()?;
    if let Some(n) = params.as_numbered_parameters_node() {
        return (n.maximum() == 1).then_some(&b"_1"[..]);
    }
    if params.as_it_parameters_node().is_some() {
        return Some(b"it");
    }
    let bp = params.as_block_parameters_node()?;
    if bp.locals().iter().next().is_some() {
        return None;
    }
    let p = bp.parameters()?;
    if p.optionals().iter().next().is_some()
        || p.rest().is_some()
        || p.posts().iter().next().is_some()
        || p.keywords().iter().next().is_some()
        || p.keyword_rest().is_some()
        || p.block().is_some()
    {
        return None;
    }
    let mut req = p.requireds().iter();
    let (Some(first), None) = (req.next(), req.next()) else { return None };
    Some(first.as_required_parameter_node()?.name().as_slice())
}

impl Cop for PredicateWithKind {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "any?" | "all?" | "none?" | "one?") {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let Some(stmts) = block.body().and_then(|b| b.as_statements_node()) else { return };
        let mut body = stmts.body().iter();
        let (Some(check), None) = (body.next(), body.next()) else { return };
        let Some(arg_name) = block_arg_name(&block) else { return };
        let Some(kind_call) = check.as_call_node() else { return };
        if is_csend(&kind_call)
            || !matches!(method_name(&kind_call), "is_a?" | "kind_of?" | "instance_of?")
            || kind_call.block().is_some_and(|b| b.as_block_node().is_some())
        {
            return;
        }
        let args = send_args(&kind_call);
        if args.len() != 1 {
            return;
        }
        let receiver_name = kind_call.receiver().and_then(|r| {
            r.as_local_variable_read_node()
                .map(|l| l.name().as_slice())
                .or_else(|| r.as_it_local_variable_read_node().map(|_| &b"it"[..]))
        });
        if receiver_name != Some(arg_name) {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let replacement = format!("{name}({})", node_text(&args[0]));
        let msg = format!("Prefer `{replacement}` to `{name} {{ ... }}` with a kind check.");
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        let sel_start = sel.start_offset();
        cx.add_fix(start, end, msg, |c| c.replace(sel_start, end, &replacement));
    }
}
