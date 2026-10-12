use crate::prelude::*;

pub const NAME: &str = "Rails/IgnoredSkipActionFilterOption";
pub static COP: IgnoredSkipActionFilterOption = IgnoredSkipActionFilterOption;
pub struct IgnoredSkipActionFilterOption;

impl Cop for IgnoredSkipActionFilterOption {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some()
            || !matches!(method_name(&call), "skip_after_action" | "skip_around_action" | "skip_before_action" | "skip_action_callback")
        {
            return;
        }
        let args = send_args(&call);
        let Some(options) = args.last().filter(|a| helpers::rails::hash_elements(a).is_some()) else { return };
        // options_hash: sym-keyed pairs, a later key overriding an earlier one.
        let mut map: Vec<(String, (usize, usize))> = Vec::new();
        for p in helpers::rails::hash_pairs(options) {
            let Some(k) = helpers::rails::sym_value(&p.key()) else { continue };
            let r = (p.location().start_offset(), p.location().end_offset());
            match map.iter_mut().find(|(n, _)| *n == k) {
                Some(slot) => slot.1 = r,
                None => map.push((k, r)),
            }
        }
        let get = |k: &str| map.iter().find(|(n, _)| n == k).map(|(_, r)| *r);
        let (target, prefer, ignore) = match (get("if"), get("only"), get("except")) {
            (Some(i), Some(_), _) => (i, "only", "if"),
            (Some(_), None, Some(x)) => (x, "if", "except"),
            _ => return,
        };
        let (s, e) = target;
        let (rs, re) = helpers::rails::left_space_and_comma(cx.src.text, s, e);
        let msg = format!("`{ignore}` option will be ignored when `{prefer}` and `{ignore}` are used together.");
        cx.add_fix(s, e, msg, |c| c.remove(rs, re));
    }
}
