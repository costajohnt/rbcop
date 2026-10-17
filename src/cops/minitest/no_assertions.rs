use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/NoAssertions";
pub static COP: NoAssertions = NoAssertions;
pub struct NoAssertions;

impl Cop for NoAssertions {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !test_class(&class) {
            return;
        }
        for case in test_cases(&class, true) {
            match case {
                TestCase::Def(d) => {
                    if assertions_count(&d.as_node()) == 0 {
                        let l = d.name_loc();
                        cx.add(l.start_offset(), l.end_offset(), "Test case has no assertions.");
                    }
                }
                TestCase::Block(c) => {
                    let n = c.as_node();
                    if descendant_sends(&n, true).iter().all(|s| !send_is_assertion(s)) {
                        let (s, e) = range(&n);
                        cx.add(s, e, "Test case has no assertions.");
                    }
                }
            }
        }
    }
}
