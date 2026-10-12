use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/NonExecutableTestMethod";
pub static COP: NonExecutableTestMethod = NonExecutableTestMethod;
pub struct NonExecutableTestMethod;

impl Cop for NonExecutableTestMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let anc = cx.ancestors;
        if def.receiver().is_some() || !test_method_shape(&def) || non_public_at(node, anc) {
            return;
        }
        let uses = *cx.state::<Option<bool>>().get_or_insert_with(|| {
            struct V(bool);
            impl<'pr> ruby_prism::Visit<'pr> for V {
                fn visit_class_node(&mut self, c: &ruby_prism::ClassNode<'pr>) {
                    self.0 |= test_class(c);
                    ruby_prism::visit_class_node(self, c);
                }
            }
            let mut v = V(false);
            ruby_prism::Visit::visit(&mut v, &anc[0]);
            v.0
        });
        if !uses || anc.iter().filter_map(|a| a.as_class_node()).any(|c| test_class(&c)) {
            return;
        }
        let (sibs, _) = helpers::visibility_help::siblings(anc, node);
        let possible = sibs.iter().filter_map(|s| s.as_class_node()).any(|c| {
            test_class(&c) && c.superclass().is_some_and(|p| node_text(&p).contains("Test"))
        });
        if possible {
            let (s, e) = range(node);
            cx.add(s, e, "Test method should be defined inside a test class to ensure execution.");
        }
    }
}
