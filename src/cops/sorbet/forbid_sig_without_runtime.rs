use crate::helpers::sorbet_signature::{Sig, signature};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidSigWithoutRuntime";
pub static COP: ForbidSigWithoutRuntime = ForbidSigWithoutRuntime;
pub struct ForbidSigWithoutRuntime;

static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"T[ \t\r\n\x0b\x0c]*::[ \t\r\n\x0b\x0c]*Sig[ \t\r\n\x0b\x0c]*::[ \t\r\n\x0b\x0c]*WithoutRuntime[ \t\r\n\x0b\x0c]*\.[ \t\r\n\x0b\x0c]*").unwrap()
});

impl Cop for ForbidSigWithoutRuntime {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if signature(node) != Some(Sig::WithoutRuntime) {
            return;
        }
        let (s, e) = send_range(&node.as_call_node().unwrap());
        let fixed = PREFIX.replace_all(cx.src.slice(s, e), "").into_owned();
        cx.add_fix(s, e, "Do not use `T::Sig::WithoutRuntime.sig`.", |c| c.replace(s, e, &fixed));
    }
}
