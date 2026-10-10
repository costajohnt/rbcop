use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateCaseCondition";
pub static COP: DuplicateCaseCondition = DuplicateCaseCondition;
pub struct DuplicateCaseCondition;

impl Cop for DuplicateCaseCondition {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_node().unwrap();
        let mut previous: Vec<Vec<String>> = Vec::new();
        for when in case.conditions().iter() {
            let Some(when) = when.as_when_node() else { continue };
            for cond in when.conditions().iter() {
                let loc = cond.location();
                let shape = helpers::node_equality::shape(cx.src, loc.start_offset(), loc.end_offset());
                if previous.contains(&shape) {
                    cx.add(loc.start_offset(), loc.end_offset(), "Duplicate `when` condition detected.");
                } else {
                    previous.push(shape);
                }
            }
        }
    }
}
