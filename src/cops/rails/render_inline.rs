use crate::prelude::*;

pub const NAME: &str = "Rails/RenderInline";
pub static COP: RenderInline = RenderInline;
pub struct RenderInline;

impl Cop for RenderInline {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :render (hash <(pair {(sym :inline) (str "inline")} _) ...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "render") {
            return;
        }
        let args = send_args(&call);
        let [hash] = args.as_slice() else { return };
        let Some(pairs) = helpers::rails::hash_elements(hash) else { return };
        let inline = pairs.iter().any(|p| {
            p.as_assoc_node().is_some_and(|a| {
                let k = a.key();
                helpers::rails::sym_value(&k).as_deref() == Some("inline")
                    || helpers::rails::str_value(&k).as_deref() == Some("inline")
            })
        });
        if inline {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Prefer using a template over inline rendering.");
        }
    }
}
