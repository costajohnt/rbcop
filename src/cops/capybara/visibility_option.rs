use crate::prelude::*;
use helpers::capybara::{self as capy, QueryType};

pub const NAME: &str = "Capybara/VisibilityOption";
pub static COP: VisibilityOption = VisibilityOption;
pub struct VisibilityOption;

impl Cop for VisibilityOption {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let queried = matches!(
            capy::query_type(name),
            Some(QueryType::Finder | QueryType::Collection | QueryType::Selector | QueryType::Match | QueryType::Action | QueryType::Grouped)
        );
        if !queried || capy::visibility_matcher_method(name) {
            return;
        }
        // (call #capybara_receiver? _ <$(hash ...) ...>)
        let mut chain: Vec<&Node> = cx.ancestors.iter().collect();
        chain.push(node);
        if !capy::capybara_receiver(call.receiver().as_ref(), &chain) {
            return;
        }
        let Some(options) = send_args(&call).into_iter().find(|a| helpers::rails::hash_elements(a).is_some()) else { return };
        if !capy::supported_visibility_query(cx, &call, &options) {
            return;
        }
        for e in helpers::rails::hash_elements(&options).unwrap() {
            // (pair (sym :visible) boolean)
            let Some(p) = e.as_assoc_node() else { continue };
            if capy::sym(cx, &p.key()).as_deref() == Some("visible")
                && (p.value().as_true_node().is_some() || p.value().as_false_node().is_some())
            {
                capy::check_visibility(cx, &p, true);
            }
        }
    }
}
