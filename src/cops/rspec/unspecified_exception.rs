use crate::prelude::*;

pub const NAME: &str = "RSpec/UnspecifiedException";
pub static COP: UnspecifiedException = UnspecifiedException;
pub struct UnspecifiedException;

impl Cop for UnspecifiedException {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !matches!(method_name(&call), "raise_exception" | "raise_error") {
            return;
        }
        if !send_args(&call).is_empty() || helpers::rspec::block_of(&call).is_some() {
            return;
        }
        // find_expect_to: the nearest `expect { ... }.to` send, not past a block
        let mut found = None;
        for a in cx.ancestors.iter().rev() {
            match kind::of(a) {
                kind::BLOCK_NODE | kind::LAMBDA_NODE => break,
                kind::CALL_NODE => {
                    let c = a.as_call_node().unwrap();
                    let expect_to = !is_csend(&c)
                        && method_name(&c) == "to"
                        && c.receiver().and_then(|r| helpers::rspec::plain_block_call(&r)).is_some_and(|e| {
                            e.receiver().is_none() && method_name(&e) == "expect" && send_args(&e).is_empty()
                        });
                    if expect_to {
                        found = Some(c);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(expect_to) = found else { return };
        // expect_to.block_node&.arguments?
        if helpers::rspec::block_of(&expect_to).is_some_and(|b| {
            b.parameters().and_then(|p| p.as_block_parameters_node()).is_some_and(|p| p.parameters().is_some() || p.locals().iter().next().is_some())
        }) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, "Specify the exception being captured");
    }
}
