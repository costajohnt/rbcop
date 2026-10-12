use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/InstanceSpy";
pub static COP: InstanceSpy = InstanceSpy;
pub struct InstanceSpy;

/// (variable, instance_double send, its selector, `.as_null_object`)
type Double = (Vec<u8>, (usize, usize), (usize, usize), (usize, usize));

impl Cop for InstanceSpy {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if !helpers::rspec::example(&l, node) {
            return;
        }
        #[derive(Default)]
        struct V {
            /// null_double: (lvasgn $_ (send $(send nil? :instance_double ...) :as_null_object))
            /// as (var, instance_double send range, selector, `.as_null_object` range)
            doubles: Vec<Double>,
            /// have_received_usage: (send (send nil? :expect (lvar $_)) :to (send nil? :have_received ...) ...)
            usages: Vec<Vec<u8>>,
        }
        impl<'pr> Visit<'pr> for V {
            fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
                if let Some(outer) = n.value().as_call_node().filter(|c| !is_csend(c) && helpers::rspec::block_of(c).is_none())
                    && method_name(&outer) == "as_null_object"
                    && send_args(&outer).is_empty()
                    && let Some(double) = outer.receiver().and_then(|r| helpers::rspec::nil_send(&r, "instance_double"))
                {
                    let sel = double.message_loc().unwrap();
                    let dot = outer.call_operator_loc().unwrap();
                    let osel = outer.message_loc().unwrap();
                    self.doubles.push((
                        n.name().as_slice().to_vec(),
                        send_range(&double),
                        (sel.start_offset(), sel.end_offset()),
                        (dot.start_offset(), osel.end_offset()),
                    ));
                }
                ruby_prism::visit_local_variable_write_node(self, n);
            }
            fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
                if !is_csend(n)
                    && method_name(n) == "to"
                    && let Some(first) = send_args(n).into_iter().next()
                    && helpers::rspec::nil_send(&first, "have_received").is_some()
                    && let Some(expect) = n.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect"))
                    && let [arg] = &send_args(&expect)[..]
                    && let Some(lvar) = arg.as_local_variable_read_node()
                {
                    self.usages.push(lvar.name().as_slice().to_vec());
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V::default();
        v.visit(node);
        for (var, (s, e), (ss, se), (ds, de)) in &v.doubles {
            for usage in &v.usages {
                if usage != var {
                    continue;
                }
                let (ss, se, ds, de) = (*ss, *se, *ds, *de);
                cx.add_fix(*s, *e, "Use `instance_spy` when you check your double with `have_received`.", |c| {
                    c.replace(ss, se, "instance_spy");
                    c.remove(ds, de);
                });
            }
        }
    }
}
