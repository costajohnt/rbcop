use crate::prelude::*;

pub const NAME: &str = "FactoryBot/FactoryClassName";
pub static COP: FactoryClassName = FactoryClassName;
pub struct FactoryClassName;

impl Cop for FactoryClassName {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send _ :factory _ (hash <(pair (sym :class) $(const ...)) ...>))
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "factory" {
            return;
        }
        let [_, hash] = &send_args(&call)[..] else { return };
        let Some(pairs) = helpers::rails::hash_elements(hash) else { return };
        let Some(class) = pairs.iter().filter_map(|p| p.as_assoc_node()).find_map(|p| {
            let k = p.key();
            (helpers::rspec::is_sym(cx, &k) && helpers::rails::sym_value(&k).as_deref() == Some("class")).then(|| p.value())
        }) else {
            return;
        };
        if !matches!(kind::of(&class), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE) {
            return;
        }
        let name = helpers::const_name::const_name(&class).unwrap_or_default();
        if matches!(name.as_str(), "Hash" | "OpenStruct") {
            return;
        }
        let (s, e) = helpers::rspec::node_range(&class);
        let replacement = format!("'{}'", node_text(&class));
        cx.add_fix(s, e, format!("Pass '{name}' string instead of `{name}` constant."), |c| c.replace(s, e, &replacement));
    }
}
