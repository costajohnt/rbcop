use crate::prelude::*;

pub const NAME: &str = "Capybara/RSpec/VisibilityMatcher";
pub static COP: VisibilityMatcher = VisibilityMatcher;
pub struct VisibilityMatcher;

const ELEMENTS: &[&str] =
    &["button", "checked_field", "css", "field", "link", "select", "selector", "table", "unchecked_field", "xpath"];

impl Cop for VisibilityMatcher {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let element = name.strip_prefix("have_no_").or_else(|| name.strip_prefix("have_"));
        if call.receiver().is_some() || !element.is_some_and(|e| ELEMENTS.contains(&e)) {
            return;
        }
        // (send nil? #capybara_matcher? ... (hash <$(pair (sym :visible) {true false}) ...>))
        let Some(pairs) = send_args(&call).last().and_then(helpers::rails::hash_elements) else { return };
        let visible = |want_true: bool| {
            pairs.iter().filter_map(|p| p.as_assoc_node()).find(|p| {
                let k = p.key();
                helpers::rspec::is_sym(cx, &k)
                    && helpers::rails::sym_value(&k).as_deref() == Some("visible")
                    && if want_true { p.value().as_true_node().is_some() } else { p.value().as_false_node().is_some() }
            })
        };
        let (f, t) = (visible(false).map(|p| helpers::rspec::node_range(&p.as_node())), visible(true).map(|p| helpers::rspec::node_range(&p.as_node())));
        if let Some((s, e)) = f {
            cx.add(s, e, "Use `:all` or `:hidden` instead of `false`.");
        }
        if let Some((s, e)) = t {
            cx.add(s, e, "Use `:visible` instead of `true`.");
        }
    }
}
