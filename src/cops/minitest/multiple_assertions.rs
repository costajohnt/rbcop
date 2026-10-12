use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/MultipleAssertions";
pub static COP: MultipleAssertions = MultipleAssertions;
pub struct MultipleAssertions;

impl Cop for MultipleAssertions {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !test_class(&class) {
            return;
        }
        let max = cx.cfg.int("Max").unwrap_or(3);
        for case in test_cases(&class, true) {
            let n = case.node();
            let body = match &case {
                TestCase::Def(d) => d.body(),
                TestCase::Block(c) => c.block().unwrap().as_block_node().unwrap().body(),
            };
            let count = parser_body(body).map_or(0, |b| multiple_assertions_count(&b)) as i64;
            if count > max {
                let (s, e) = range(&n);
                cx.add(s, e, format!("Test case has too many assertions [{count}/{max}]."));
            }
        }
    }
}
