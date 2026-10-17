use crate::prelude::*;

pub const NAME: &str = "RSpec/BeforeAfterAll";
pub static COP: BeforeAfterAll = BeforeAfterAll;
pub struct BeforeAfterAll;

impl Cop for BeforeAfterAll {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // $(send _ RESTRICT_ON_SEND (sym {:all :context}))
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "before" | "after") {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        if !helpers::rspec::is_sym(cx, arg) || !matches!(helpers::rails::sym_value(arg).as_deref(), Some("all" | "context")) {
            return;
        }
        let (s, e) = send_range(&call);
        let hook = cx.src.slice(s, e);
        cx.add(
            s,
            e,
            format!(
                "Beware of using `{hook}` as it may cause state to leak between tests. If you are using \
                 `rspec-rails`, and `use_transactional_fixtures` is enabled, then records created in `{hook}` \
                 are not automatically rolled back."
            ),
        );
    }
}
