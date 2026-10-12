use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/TestFileName";
pub static COP: TestFileName = TestFileName;
pub struct TestFileName;

impl Cop for TestFileName {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        struct V(bool);
        impl<'pr> ruby_prism::Visit<'pr> for V {
            fn visit_class_node(&mut self, c: &ruby_prism::ClassNode<'pr>) {
                self.0 |= test_class(c);
                ruby_prism::visit_class_node(self, c);
            }
        }
        let program = node.as_program_node().unwrap();
        if program.statements().body().iter().next().is_none() {
            return;
        }
        let mut v = V(false);
        ruby_prism::Visit::visit(&mut v, node);
        if !v.0 {
            return;
        }
        let base = std::path::Path::new(cx.src.path).file_name().map(|b| b.to_string_lossy().into_owned()).unwrap_or_default();
        if !(base.starts_with("test_") || base.ends_with("_test.rb")) {
            cx.add_global("Test file path should start with `test_` or end with `_test.rb`.");
        }
    }
}
