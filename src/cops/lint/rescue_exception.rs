use crate::prelude::*;

pub const NAME: &str = "Lint/RescueException";
pub static COP: RescueException = RescueException;
pub struct RescueException;

/// `const_name == 'Exception'`: `Exception` or `::Exception`.
fn targets_exception(node: &Node) -> bool {
    if let Some(c) = node.as_constant_read_node() {
        return c.name().as_slice() == b"Exception";
    }
    node.as_constant_path_node()
        .is_some_and(|p| p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Exception"))
}

impl Cop for RescueException {
    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let r = node.as_rescue_node().unwrap();
        if r.exceptions().iter().any(|e| targets_exception(&e)) {
            let (s, e) = helpers::resbody::range(cx.src, &r);
            cx.add(s, e, "Avoid rescuing the `Exception` class. Perhaps you meant to rescue `StandardError`?");
        }
    }
}
