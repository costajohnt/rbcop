use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/ReturnFromStub";
pub static COP: ReturnFromStub = ReturnFromStub;
pub struct ReturnFromStub;

/// A replacement: range and text.
type Edit = (usize, usize, String);

/// The parts of the send a call stands for (a block around it excluded).
fn visit_send<'pr>(v: &mut impl Visit<'pr>, call: &CallNode<'pr>) {
    if let Some(r) = call.receiver() {
        v.visit(&r);
    }
    for a in send_args(call) {
        v.visit(&a);
    }
}

fn dynamic(node: Option<&Node>) -> bool {
    node.is_some_and(|n| !helpers::rspec::recursive_literal_or_const(n))
}

impl Cop for ReturnFromStub {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let block_style = cx.cfg.str("EnforcedStyle") == Some("block");
        if block_style {
            // on_send and_return
            if is_csend(&call) || method_name(&call) != "and_return" {
                return;
            }
            let mut s = StubIn(false);
            visit_send(&mut s, &call);
            if !s.0 {
                return;
            }
            // and_return_value: $(send _ :and_return $(...)) in the send, with exactly one argument
            #[derive(Default)]
            struct A(Vec<((usize, usize), Option<Edit>)>);
            impl<'pr> Visit<'pr> for A {
                fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                    check(self, n);
                    ruby_prism::visit_call_node(self, n);
                }
            }
            fn check(a: &mut A, n: &CallNode) {
                if is_csend(n) || method_name(n) != "and_return" {
                    return;
                }
                let [arg] = &send_args(n)[..] else { return };
                if dynamic(Some(arg)) {
                    return;
                }
                let sel = n.message_loc().unwrap();
                // AndReturnCallCorrector: from the receiver's end to the send's end
                let fix = n.receiver().filter(|_| !is_heredoc(arg)).map(|r| {
                    let src = node_text(arg);
                    let rep = if arg.as_keyword_hash_node().is_some() { format!("{{ {src} }}") } else { src.to_string() };
                    (r.location().end_offset(), send_range(n).1, format!(" {{ {rep} }}"))
                });
                // A missing receiver raises in the corrector: no offense
                if n.receiver().is_none() && !is_heredoc(arg) {
                    return;
                }
                a.0.push(((sel.start_offset(), sel.end_offset()), fix));
            }
            let mut a = A::default();
            check(&mut a, &call);
            visit_send(&mut a, &call);
            for ((s, e), fix) in a.0 {
                cx.add_fix(s, e, "Use block for static values.", |c| {
                    if let Some((fs, fe, r)) = &fix {
                        c.replace(*fs, *fe, r);
                    }
                });
            }
            return;
        }
        if cx.cfg.str("EnforcedStyle") != Some("and_return") {
            return;
        }
        // on_block: (block #contains_stub? ...), the stub in the block's send
        let Some(block) = helpers::rspec::plain_block_call(node).and_then(|c| helpers::rspec::block_of(&c)) else { return };
        let mut s = StubIn(false);
        visit_send(&mut s, &call);
        if !(s.0 || (call.receiver().is_none() && method_name(&call) == "receive" && send_args(&call).len() == 1)) {
            return;
        }
        let body = helpers::rspec::parser_body(block.body());
        if dynamic(body.as_ref()) {
            return;
        }
        let open = block.opening_loc();
        let (s, e) = (open.start_offset(), open.end_offset());
        let fix = (!body.as_ref().is_some_and(is_heredoc)).then(|| {
            let (ss, se) = send_range(&call);
            let body_src = body.as_ref().map_or("nil".to_string(), |b| {
                let (bs, be) = body_range(b);
                cx.src.slice(bs, be).to_string()
            });
            let (ns, ne) = helpers::rspec::node_range(node);
            (ns, ne, format!("{}.and_return({body_src})", cx.src.slice(ss, se)))
        });
        cx.add_fix(s, e, "Use `and_return` for static values.", |c| {
            if let Some((fs, fe, r)) = &fix {
                c.replace(*fs, *fe, r);
            }
        });
    }
}

struct StubIn(bool);
impl<'pr> Visit<'pr> for StubIn {
    fn visit_call_node(&mut self, n: &CallNode<'pr>) {
        self.0 |= n.receiver().is_none() && method_name(n) == "receive" && send_args(n).len() == 1;
        ruby_prism::visit_call_node(self, n);
    }
}
