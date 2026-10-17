use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/NonPublicTestMethod";
pub static COP: NonPublicTestMethod = NonPublicTestMethod;
pub struct NonPublicTestMethod;

impl Cop for NonPublicTestMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        let (body, idx) = class_def_nodes(&class);
        for i in idx {
            let d = body[i].as_def_node().unwrap();
            if test_method_shape(&d) && non_public_in(&body, i) && !assertions(&d).is_empty() {
                let (s, e) = range(&body[i]);
                cx.add(s, e, "Non `public` test method detected. Make it `public` for it to run.");
            }
        }
        // Test blocks: only those that are class body statements can follow a visibility modifier.
        for b in test_blocks(node) {
            let bn = b.as_node();
            let Some(i) = body.iter().position(|n| range(n) == range(&bn)) else { continue };
            let sibs: &[Node] = if body.len() > 1 { &body } else { &[] };
            if helpers::visibility_help::node_visibility(&bn, None, sibs, i) == "public" {
                continue;
            }
            let block_body = b.block().unwrap().as_block_node().unwrap().body();
            if !body_assertions(block_body).is_empty() {
                let (s, e) = range(&bn);
                cx.add(s, e, "Non `public` test method detected. Make it `public` for it to run.");
            }
        }
    }
}
