use crate::prelude::*;
use helpers::literal_node::{parser_type, recursive_basic_literal};

pub const NAME: &str = "Style/HashLikeCase";
pub static COP: HashLikeCase = HashLikeCase;
pub struct HashLikeCase;

/// A `when` body as parser sees it: one statement, or a `begin` of several.
fn literal_body_type(when: &ruby_prism::WhenNode) -> Option<&'static str> {
    let body: Vec<Node> = when.statements()?.body().iter().collect();
    match body.as_slice() {
        [one] => recursive_basic_literal(one).then(|| parser_type(one)),
        many => many.iter().all(recursive_basic_literal).then_some("begin"),
    }
}

impl Cop for HashLikeCase {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let case = node.as_case_node().unwrap();
        let min = cx.cfg.positive_int("MinBranchesCount", 3);
        if case.conditions().iter().count() < min || case.else_clause().is_some() {
            return;
        }
        let mut conds = Vec::new();
        let mut bodies = Vec::new();
        for w in case.conditions().iter() {
            let w = w.as_when_node().unwrap();
            let c: Vec<Node> = w.conditions().iter().collect();
            let [c] = c.as_slice() else { return };
            let t = parser_type(c);
            if t != "str" && t != "sym" {
                return;
            }
            let Some(b) = literal_body_type(&w) else { return };
            conds.push(t);
            bodies.push(b);
        }
        if conds.iter().all(|t| *t == conds[0]) && bodies.iter().all(|t| *t == bodies[0]) {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), "Consider replacing `case-when` with a hash lookup.");
        }
    }
}
