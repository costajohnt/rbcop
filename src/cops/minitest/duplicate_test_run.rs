use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/DuplicateTestRun";
pub static COP: DuplicateTestRun = DuplicateTestRun;
pub struct DuplicateTestRun;

impl Cop for DuplicateTestRun {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !test_class(&class) || test_cases(&class, true).is_empty() {
            return;
        }
        let Some(parent_class) = class.superclass() else { return };
        let Some(parent) = helpers::parser_parent::parser_parent(cx.ancestors) else { return };
        // parent.each_child_node(:class)
        let siblings: Vec<Node> = match parent.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => {
                let body = match parent {
                    Node::ModuleNode { .. } => parent.as_module_node().unwrap().body(),
                    Node::ClassNode { .. } => parent.as_class_node().unwrap().body(),
                    Node::SingletonClassNode { .. } => parent.as_singleton_class_node().unwrap().body(),
                    _ => None,
                };
                parser_body(body).into_iter().collect()
            }
        };
        let target = siblings.iter().filter_map(|n| n.as_class_node()).find(|k| {
            let id = k.constant_path();
            helpers::node_equality::same(cx.src, range(&id), range(&parent_class))
        });
        if target.is_some_and(|k| !test_cases(&k, true).is_empty()) {
            let (s, e) = range(node);
            cx.add(s, e, "Subclasses with test methods causes the parent' tests to run them twice.");
        }
    }
}
