use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/IteratedExpectation";
pub static COP: IteratedExpectation = IteratedExpectation;
pub struct IteratedExpectation;

/// `(lvar %)`: a read of the block argument (`it` reads included for an itblock).
fn is_lvar(node: &Node, arg: &[u8]) -> bool {
    node.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == arg)
        || (arg == b"it" && node.as_it_local_variable_read_node().is_some())
}

/// expectation?: `(send (send nil? :expect (lvar %)) :to ...)`
fn expectation(node: &Node, arg: &[u8]) -> bool {
    let Some(to) = helpers::rspec::send_call(node).filter(|c| !is_csend(c) && method_name(c) == "to") else { return false };
    let Some(expect) = to.receiver().and_then(|r| helpers::rspec::nil_send(&r, "expect")) else { return false };
    matches!(&send_args(&expect)[..], [a] if is_lvar(a, arg))
}

impl Cop for IteratedExpectation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send ... :each) (args (arg $_)) (...)) and the numblock/itblock forms
        let call = node.as_call_node().unwrap();
        let Some(block) = helpers::rspec::block_of(&call) else { return };
        if is_csend(&call) || method_name(&call) != "each" || !send_args(&call).is_empty() {
            return;
        }
        let arg: Vec<u8> = match block.parameters() {
            Some(p) if p.as_numbered_parameters_node().is_some() => b"_1".to_vec(),
            Some(p) if p.as_it_parameters_node().is_some() => b"it".to_vec(),
            Some(p) => {
                let Some(bp) = p.as_block_parameters_node() else { return };
                if bp.locals().iter().next().is_some() {
                    return;
                }
                let Some(params) = bp.parameters() else { return };
                let only = params.requireds().iter().count() == 1
                    && params.optionals().iter().count() == 0
                    && params.rest().is_none()
                    && params.posts().iter().count() == 0
                    && params.keywords().iter().count() == 0
                    && params.keyword_rest().is_none()
                    && params.block().is_none();
                let Some(r) = params.requireds().iter().next().filter(|_| only).and_then(|r| r.as_required_parameter_node()) else {
                    return;
                };
                r.name().as_slice().to_vec()
            }
            None => return,
        };
        let Some(body) = helpers::rspec::parser_body(block.body()) else { return };
        let msg = "Prefer using the `all` matcher instead of iterating over an array.";
        let (s, e) = send_range(&call);
        if expectation(&body, &arg) {
            let to = body.as_call_node().unwrap();
            let to_args = send_args(&to);
            // the corrector: one matcher that does not use the argument, and a receiver
            let fix = match &to_args[..] {
                [matcher] => {
                    struct V<'a>(&'a [u8], usize);
                    impl<'pr> Visit<'pr> for V<'_> {
                        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
                            self.1 += usize::from(n.name().as_slice() == self.0);
                        }
                        fn visit_it_local_variable_read_node(&mut self, _: &ruby_prism::ItLocalVariableReadNode<'pr>) {
                            self.1 += usize::from(self.0 == b"it");
                        }
                    }
                    let mut v = V(&arg, 0);
                    v.visit(matcher);
                    // each_descendant: the matcher's own node does not count
                    let uses = v.1 > usize::from(is_lvar(matcher, &arg));
                    match call.receiver() {
                        _ if uses => Some(None),
                        Some(recv) => Some(Some(format!("expect({}).to all({})", node_text(&recv), node_text(matcher)))),
                        None => None,
                    }
                }
                _ => Some(None),
            };
            // `node.receiver.source` raises without a receiver: no offense
            let Some(fix) = fix else { return };
            let (ns, ne) = helpers::rspec::node_range(node);
            cx.add_fix(s, e, msg, |c| {
                if let Some(r) = &fix {
                    c.replace(ns, ne, r);
                }
            });
            return;
        }
        // only_expectations?: every child of the body is an expectation
        if let Some(children) = helpers::rspec::begin_children(&body)
            && !children.is_empty()
            && children.iter().all(|c| expectation(c, &arg))
        {
            cx.add_with(s, e, msg, false);
        }
    }
}
