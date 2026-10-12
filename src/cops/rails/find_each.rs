use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/FindEach";
pub static COP: FindEach = FindEach;
pub struct FindEach;

const SCOPE_METHODS: &[&str] = &[
    "all", "eager_load", "includes", "joins", "left_joins", "left_outer_joins", "not", "or", "preload", "references",
    "unscoped", "where",
];

impl Cop for FindEach {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "each" {
            return;
        }
        let Some(recv) = call.receiver().and_then(|r| helpers::rails::send_node(&r)) else { return };
        if !SCOPE_METHODS.contains(&method_name(&recv)) {
            return;
        }
        if recv.receiver().is_none() && !helpers::rails::inherit_active_record_base(cx.ancestors) {
            return;
        }
        // active_model_error_where?: `errors.where(...)`.
        if method_name(&recv) == "where"
            && recv.receiver().and_then(|r| helpers::rails::send_node(&r)).is_some_and(|r| method_name(&r) == "errors")
        {
            return;
        }
        // node.each_node(:send): every send in the `each` send (its block excluded).
        struct Sends(Vec<String>);
        impl<'pr> Visit<'pr> for Sends {
            fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
                if !is_csend(c) {
                    self.0.push(method_name(c).to_string());
                }
                ruby_prism::visit_call_node(self, c);
            }
        }
        let mut sends = Sends(vec!["each".to_string()]);
        if let Some(r) = call.receiver() {
            sends.visit(&r);
        }
        for a in send_args(&call) {
            sends.visit(&a);
        }
        if sends
            .0
            .iter()
            .any(|m| helpers::allowed::allowed_method(cx.cfg, m) || helpers::allowed::matches_allowed_pattern(cx.cfg, m))
        {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, "Use `find_each` instead of `each`.", |c| c.replace(s, e, "find_each"));
    }
}
