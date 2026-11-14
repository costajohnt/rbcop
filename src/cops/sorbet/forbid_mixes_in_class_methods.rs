use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidMixesInClassMethods";
pub static COP: ForbidMixesInClassMethods = ForbidMixesInClassMethods;
pub struct ForbidMixesInClassMethods;

impl Cop for ForbidMixesInClassMethods {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send {self | nil? | (const (const {cbase | nil?} :T) :Helpers)} :mixes_in_class_methods ...)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "mixes_in_class_methods" || is_csend(&call) {
            return;
        }
        let receiver_ok = match call.receiver() {
            None => true,
            Some(r) => {
                r.as_self_node().is_some()
                    || r.as_constant_path_node().is_some_and(|p| {
                        p.name().is_some_and(|n| n.as_slice() == b"Helpers") && p.parent().is_some_and(|t| is_top_const(&t, "T"))
                    })
            }
        };
        if receiver_ok {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Do not use `mixes_in_class_methods`, use `extend ActiveSupport::Concern` instead.");
        }
    }
}
