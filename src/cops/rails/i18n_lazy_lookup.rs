use crate::prelude::*;
use helpers::parser_parent::parser_parent_index;

pub const NAME: &str = "Rails/I18nLazyLookup";
pub static COP: I18nLazyLookup = I18nLazyLookup;
pub struct I18nLazyLookup;

impl Cop for I18nLazyLookup {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? {:translate :t} ${sym_type? str_type?} ...)
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() || !matches!(method_name(&call), "translate" | "t") {
            return;
        }
        let args = send_args(&call);
        let Some(key_node) = args.first() else { return };
        let is_sym = helpers::rails::sym_value(key_node).is_some();
        let Some(key) = helpers::rails::str_or_sym(key_node) else { return };
        let lazy = cx.cfg.str("EnforcedStyle") != Some("explicit");
        if lazy == key.starts_with('.') {
            return;
        }
        // controller_and_action: the nearest def (public) and class (named ...Controller).
        let anc = cx.ancestors;
        let Some(di) = anc.iter().rposition(|a| a.as_def_node().is_some_and(|d| d.receiver().is_none())) else { return };
        let def = anc[di].as_def_node().unwrap();
        let parent = parser_parent_index(anc, di).map(|p| &anc[p]);
        let (sibs, idx) = helpers::visibility_help::siblings(&anc[..di], &anc[di]);
        if helpers::visibility_help::node_visibility(&anc[di], parent, &sibs, idx) != "public" {
            return;
        }
        let Some(ci) = anc.iter().rposition(|a| a.as_class_node().is_some()) else { return };
        let class = anc[ci].as_class_node().unwrap();
        let cp = class.constant_path();
        let controller_name = node_text(&cp);
        if !controller_name.ends_with("Controller") {
            return;
        }
        // controller_path: (parent_module_name::)Name, minus `Controller`, underscored.
        let module = helpers::rails::parent_module_name(&anc[..ci], None);
        let path = match module.as_deref() {
            Some("Object") => controller_name.to_string(),
            Some(m) => format!("{m}::{controller_name}"),
            None => format!("::{controller_name}"),
        };
        let path = helpers::inflector::underscore(path.strip_suffix("Controller").unwrap_or(&path)).replace('/', ".");
        let action = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        // `key.to_s.split('.').last`
        let last = helpers::rails::ruby_split(&key, '.').last().copied().unwrap_or("").to_string();
        let scoped = format!("{path}.{action}.{last}");
        let (s, e) = (key_node.location().start_offset(), key_node.location().end_offset());
        let style = if lazy { "lazy" } else { "explicit" };
        let msg = format!("Use {style} lookup for the text used in controllers.");
        let replacement = if lazy {
            // `key == scoped_key`: a Symbol key never equals the String.
            if is_sym || key != scoped {
                return;
            }
            format!("'.{last}'")
        } else {
            format!("'{scoped}'")
        };
        // Deliberate difference: the key text is spliced into a '...' literal only when plain.
        if !helpers::rails::inert_text(&replacement[1..replacement.len() - 1]) {
            return cx.add_withheld_fix(s, e, msg);
        }
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
