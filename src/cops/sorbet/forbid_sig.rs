use crate::helpers::sorbet_signature::{Sig, signature};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidSig";
pub static COP: ForbidSig = ForbidSig;
pub struct ForbidSig;

impl Cop for ForbidSig {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node) == Some(Sig::Bare) {
            let (s, e) = helpers::node_types::range(node);
            cx.add(s, e, "Do not use `T::Sig`.");
        }
    }
}
