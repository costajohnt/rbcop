use crate::prelude::*;

pub const NAME: &str = "Rails/DelegateAllowBlank";
pub static COP: DelegateAllowBlank = DelegateAllowBlank;
pub struct DelegateAllowBlank;

impl Cop for DelegateAllowBlank {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :delegate _ (hash <$(pair (sym :allow_blank) true) ...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "delegate") {
            return;
        }
        let args = send_args(&call);
        let [_, hash] = args.as_slice() else { return };
        let Some(pairs) = helpers::rails::hash_elements(hash) else { return };
        let Some(pair) = pairs.iter().find_map(|p| {
            let a = p.as_assoc_node()?;
            (helpers::rails::sym_value(&a.key()).as_deref() == Some("allow_blank") && a.value().as_true_node().is_some())
                .then_some(a)
        }) else {
            return;
        };
        let (s, e) = (pair.location().start_offset(), pair.location().end_offset());
        let (ks, ke) = helpers::rails::sym_range(&pair.key());
        cx.add_fix(s, e, "`allow_blank` is not a valid option, use `allow_nil`.", |c| c.replace(ks, ke, "allow_nil"));
    }
}
