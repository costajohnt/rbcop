use crate::prelude::*;

pub const NAME: &str = "Performance/AncestorsInclude";
pub static COP: AncestorsInclude = AncestorsInclude;
pub struct AncestorsInclude;

impl Cop for AncestorsInclude {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send $_subclass :ancestors) :include? $_superclass)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "include?" {
            return;
        }
        let args = send_args(&call);
        let [superclass] = args.as_slice() else { return };
        let Some(ancestors) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        if is_csend(&ancestors) || method_name(&ancestors) != "ancestors" || !send_args(&ancestors).is_empty() {
            return;
        }
        if ancestors.block().is_some() {
            return;
        }
        let subclass = ancestors.receiver();
        if subclass.as_ref().is_some_and(|s| s.as_constant_read_node().is_none() && s.as_constant_path_node().is_none()) {
            return;
        }
        let start = ancestors.message_loc().unwrap().start_offset();
        let end = call.message_loc().unwrap().end_offset();
        let replacement = format!("{} <= {}", subclass.as_ref().map_or("self", |s| node_text(s)), node_text(superclass));
        let (s, e) = send_range(&call);
        cx.add_fix(start, end, "Use `<=` instead of `ancestors.include?`.", |c| c.replace(s, e, &replacement));
    }
}
