use crate::prelude::*;

pub const NAME: &str = "Lint/RandOne";
pub static COP: RandOne = RandOne;
pub struct RandOne;

/// `(int {-1 1})` / `(float {-1.0 1.0})`
fn one(node: &Node) -> bool {
    if let Some(i) = node.as_integer_node() {
        let v: Result<i32, _> = i.value().try_into();
        return matches!(v, Ok(1 | -1));
    }
    node.as_float_node().is_some_and(|f| f.value().abs() == 1.0)
}

/// `(const {nil? cbase} :Kernel)`
fn kernel(node: &Node) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        return c.name().as_slice() == b"Kernel";
    }
    node.as_constant_path_node()
        .is_some_and(|p| p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Kernel"))
}

impl Cop for RandOne {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "rand" || is_csend(&call) || !call.receiver().is_none_or(|r| kernel(&r)) {
            return;
        }
        let args = send_args(&call);
        if args.len() != 1 || !one(&args[0]) {
            return;
        }
        let (start, end) = send_range(&call);
        let msg = format!("`{}` always returns `0`. Perhaps you meant `rand(2)` or `rand`?", cx.src.slice(start, end));
        cx.add(start, end, msg);
    }
}
