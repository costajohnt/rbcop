use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/TestMethodName";
pub static COP: TestMethodName = TestMethodName;
pub struct TestMethodName;

impl Cop for TestMethodName {
    fn correctable(&self) -> bool {
        true
    }

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
            let d = body[i].as_def_node().unwrap();
            let name = String::from_utf8_lossy(d.name().as_slice()).into_owned();
            if assertions(&d).is_empty()
                || non_public_in(&body, i)
                || d.parameters().is_some()
                || name.starts_with("test_")
                || lifecycle_hook(&body[i])
            {
                continue;
            }
            let l = d.name_loc();
            let (s, e) = (l.start_offset(), l.end_offset());
            let new = format!("test_{name}");
            cx.add_fix(s, e, "Test method name should start with `test_` prefix.", |c| c.replace(s, e, &new));
        }
    }
}
