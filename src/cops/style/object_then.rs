use crate::prelude::*;

pub const NAME: &str = "Style/ObjectThen";
pub static COP: ObjectThen = ObjectThen;
pub struct ObjectThen;

impl Cop for ObjectThen {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.6, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "then" | "yield_self") {
            return;
        }
        // on_block (any block literal) or on_send with a single `&blk` argument.
        let ok = match call.block() {
            Some(b) if b.as_block_node().is_some() => true,
            Some(_) => call.arguments().is_none(),
            None => false,
        };
        if !ok {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("then").to_string();
        if name == style {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let prefer = if style == "then" && call.receiver().is_none() { "self.then".to_string() } else { style.clone() };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Prefer `{style}` over `{name}`."), |c| c.replace(s, e, &prefer));
    }
}
