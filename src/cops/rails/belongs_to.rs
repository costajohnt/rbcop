use crate::prelude::*;

pub const NAME: &str = "Rails/BelongsTo";
pub static COP: BelongsTo = BelongsTo;
pub struct BelongsTo;

impl Cop for BelongsTo {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ :belongs_to ... (hash <$(pair (sym :required) $boolean) ...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "belongs_to" {
            return;
        }
        let args = send_args(&call);
        let Some(hash) = args.last().filter(|h| helpers::rails::hash_elements(h).is_some()) else { return };
        let Some(pair) = helpers::rails::hash_pairs(hash).into_iter().find(|p| {
            helpers::rails::sym_value(&p.key()).as_deref() == Some("required")
                && (p.value().as_true_node().is_some() || p.value().as_false_node().is_some())
        }) else {
            return;
        };
        let (msg, replacement) = if pair.value().as_true_node().is_some() {
            (
                "You specified `required: true`, in Rails > 5.0 the required option is deprecated and you want to use `optional: false`. In most configurations, this is the default and you can omit this option altogether",
                "optional: false",
            )
        } else {
            (
                "You specified `required: false`, in Rails > 5.0 the required option is deprecated and you want to use `optional: true`.",
                "optional: true",
            )
        };
        let sel = call.message_loc().unwrap();
        let (ps, pe) = (pair.location().start_offset(), pair.location().end_offset());
        cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| c.replace(ps, pe, replacement));
    }
}
