use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/NoTestCases";
pub static COP: NoTestCases = NoTestCases;
pub struct NoTestCases;

impl Cop for NoTestCases {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if test_class(&class) && test_cases(&class, true).is_empty() {
            let (s, e) = range(node);
            cx.add(s, e, "Test class should have test cases.");
        }
    }
}
