use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/TableNameAssignment";
pub static COP: TableNameAssignment = TableNameAssignment;
pub struct TableNameAssignment;

impl Cop for TableNameAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        // (class (const ... :Base) ...)
        if helpers::rails::const_named(&class.constant_path(), "Base") {
            return;
        }
        for (s, e) in find_set_table_name(node) {
            cx.add(s, e, "Do not use `self.table_name =`.");
        }
    }
}

/// ActiveRecordHelper#find_set_table_name: `(send self :table_name= {str sym})` anywhere below.
pub fn find_set_table_name(node: &Node) -> Vec<(usize, usize)> {
    struct F(Vec<(usize, usize)>);
    impl<'pr> Visit<'pr> for F {
        fn visit_call_node(&mut self, call: &ruby_prism::CallNode<'pr>) {
            let args = send_args(call);
            if !is_csend(call)
                && method_name(call) == "table_name="
                && call.receiver().is_some_and(|r| r.as_self_node().is_some())
                && args.len() == 1
                && helpers::rails::str_or_sym(&args[0]).is_some()
            {
                self.0.push(send_range(call));
            }
            ruby_prism::visit_call_node(self, call);
        }
    }
    let mut f = F(Vec::new());
    f.visit(node);
    f.0
}
