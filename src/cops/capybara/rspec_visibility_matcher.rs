use crate::prelude::*;
use helpers::capybara as capy;

pub const NAME: &str = "Capybara/RSpec/VisibilityMatcher";
pub static COP: VisibilityMatcher = VisibilityMatcher;
pub struct VisibilityMatcher;

impl Cop for VisibilityMatcher {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if call.receiver().is_some() || !capy::visibility_matcher_method(name) {
            return;
        }
        // (send nil? #capybara_matcher? ... (hash <$(pair (sym :visible) {true false}) ...>))
        let args = send_args(&call);
        let Some(hash) = args.last() else { return };
        let Some(pairs) = helpers::rails::hash_elements(hash) else { return };
        let visible = |want_true: bool| {
            pairs.iter().filter_map(|p| p.as_assoc_node()).find(|p| {
                capy::sym(cx, &p.key()).as_deref() == Some("visible")
                    && if want_true { p.value().as_true_node().is_some() } else { p.value().as_false_node().is_some() }
            })
        };
        let (f, t) = (visible(false), visible(true));
        if let Some(p) = f {
            capy::check_visibility(cx, &p, false);
        }
        if let Some(p) = t {
            // autocorrectable?: an unknown first argument to a generic selector matcher may be a
            // custom selector name
            let first_ok = capy::query_selector_kind(name) != Some("selector")
                || args.first().is_some_and(|f| {
                    helpers::rails::hash_elements(f).is_some()
                        || helpers::rspec::is_sym(cx, f)
                        || helpers::rspec::is_str(f)
                        || helpers::rspec::is_dstr(f)
                });
            let correct = first_ok && capy::supported_visibility_query(cx, &call, hash);
            capy::check_visibility(cx, &p, correct);
        }
    }
}
