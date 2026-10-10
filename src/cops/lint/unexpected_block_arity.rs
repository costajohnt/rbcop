use crate::prelude::*;

pub const NAME: &str = "Lint/UnexpectedBlockArity";
pub static COP: UnexpectedBlockArity = UnexpectedBlockArity;
pub struct UnexpectedBlockArity;

/// `arg_count`: positional parameters (`arg`, `optarg`, `mlhs`); a splat makes it unbounded.
fn arg_count(block: &ruby_prism::BlockNode) -> i64 {
    let Some(params) = block.parameters() else { return 0 };
    if let Some(n) = params.as_numbered_parameters_node() {
        return n.maximum() as i64;
    }
    if params.as_it_parameters_node().is_some() {
        return 1;
    }
    let Some(p) = params.as_block_parameters_node().and_then(|b| b.parameters()) else { return 0 };
    // `|a,|` gives Prism an ImplicitRestNode that parser has no argument for.
    if p.rest().is_some_and(|r| r.as_rest_parameter_node().is_some()) {
        return i64::MAX;
    }
    (p.requireds().iter().count() + p.optionals().iter().count() + p.posts().iter().count()) as i64
}

impl Cop for UnexpectedBlockArity {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if call.receiver().is_none() {
            return;
        }
        let name = method_name(&call);
        let Some(Value::Map(methods)) = cx.cfg.get("Methods") else { return };
        let Some(Value::Int(expected)) = methods.get(name) else { return };
        let expected = *expected;
        let actual = arg_count(&block);
        if actual >= expected {
            return;
        }
        let loc = node.location();
        cx.add(
            loc.start_offset(),
            loc.end_offset(),
            format!("`{name}` expects at least {expected} positional arguments, got {actual}."),
        );
    }
}
