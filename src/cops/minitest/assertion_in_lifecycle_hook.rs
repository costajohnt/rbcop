use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/AssertionInLifecycleHook";
pub static COP: AssertionInLifecycleHook = AssertionInLifecycleHook;
pub struct AssertionInLifecycleHook;

impl Cop for AssertionInLifecycleHook {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !test_class(&class) {
            return;
        }
        let (body, idx) = class_def_nodes(&class);
        for i in idx {
            if !lifecycle_hook(&body[i]) {
                continue;
            }
            let d = body[i].as_def_node().unwrap();
            let hook = String::from_utf8_lossy(d.name().as_slice()).into_owned();
            for send in descendant_sends(&body[i], false) {
                if send_is_assertion(&send) {
                    let (s, e) = send_range(&send);
                    cx.add(s, e, format!("Do not use `{}` in `{hook}` hook.", method_name(&send)));
                }
            }
        }
    }
}
