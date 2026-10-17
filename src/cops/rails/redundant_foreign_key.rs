use crate::prelude::*;

pub const NAME: &str = "Rails/RedundantForeignKey";
pub static COP: RedundantForeignKey = RedundantForeignKey;
pub struct RedundantForeignKey;

impl Cop for RedundantForeignKey {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let ty = method_name(&call);
        if call.receiver().is_some() || !matches!(ty, "belongs_to" | "has_one" | "has_many" | "has_and_belongs_to_many") {
            return;
        }
        let args = send_args(&call);
        let [name, options] = args.as_slice() else { return };
        let Some(name) = helpers::rails::str_or_sym(name) else { return };
        if helpers::rails::hash_elements(options).is_none() {
            return;
        }
        let Some(pair) = helpers::rails::hash_pair(options, "foreign_key") else { return };
        let Some(foreign_key) = helpers::rails::str_or_sym(&pair.value()) else { return };
        let default = if ty == "belongs_to" {
            Some(format!("{name}_id"))
        } else if let Some(as_pair) = helpers::rails::hash_pairs(options)
            .into_iter()
            .find(|p| helpers::rails::sym_value(&p.key()).as_deref() == Some("as"))
        {
            // `&.value&.value`: a value node without `value` raises.
            let v = as_pair.value();
            match helpers::rails::str_or_sym(&v) {
                Some(s) => Some(format!("{s}_id")),
                None if v.as_nil_node().is_some() => Some("_id".to_string()),
                None => return,
            }
        } else {
            helpers::rails::parent_module_name(cx.ancestors, Some(&call)).map(|m| helpers::inflector::foreign_key(&m))
        };
        if default.as_deref() != Some(foreign_key.as_str()) {
            return;
        }
        let src = cx.src;
        let (ps, pe) = (pair.location().start_offset(), pair.location().end_offset());
        let (rs, re) = helpers::rails::left_space_and_comma(src.text, ps, pe);
        cx.add_fix(ps, pe, "Specifying the default value for `foreign_key` is redundant.", |c| c.remove(rs, re));
    }
}
