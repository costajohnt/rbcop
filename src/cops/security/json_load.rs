use crate::prelude::*;

pub const NAME: &str = "Security/JSONLoad";
pub static COP: JSONLoad = JSONLoad;
pub struct JSONLoad;

impl Cop for JSONLoad {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if is_csend(&call) || !matches!(method, "load" | "restore") {
            return;
        }
        if call.receiver().is_none_or(|r| helpers::const_name::const_name(&r).as_deref() != Some("JSON")) {
            return;
        }
        // { _ hash | ... !`(pair (sym :create_additions) _) }: options as the second argument are
        // always reported (json < 2.17 ignores them); otherwise the last argument has no such pair
        // anywhere inside.
        let args = send_args(&call);
        let hash_second =
            args.len() == 2 && (args[1].as_hash_node().is_some() || args[1].as_keyword_hash_node().is_some());
        if !hash_second && args.last().is_none_or(has_create_additions) {
            return;
        }
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Prefer `JSON.parse` over `JSON.{method}`."), |c| c.replace(s, e, "parse"));
    }
}

struct Finder(bool);

impl<'pr> ruby_prism::Visit<'pr> for Finder {
    fn visit_assoc_node(&mut self, node: &ruby_prism::AssocNode<'pr>) {
        if node.key().as_symbol_node().is_some_and(|s| s.unescaped() == b"create_additions") {
            self.0 = true;
        }
        ruby_prism::visit_assoc_node(self, node);
    }
}

fn has_create_additions(node: &Node) -> bool {
    let mut f = Finder(false);
    ruby_prism::Visit::visit(&mut f, node);
    f.0
}
