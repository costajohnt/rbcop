use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/RedundantReceiverInWithOptions";
pub static COP: RedundantReceiverInWithOptions = RedundantReceiverInWithOptions;
pub struct RedundantReceiverInWithOptions;

/// The `send` nodes (pre-order) and whether a parser `block` (a non-numbered block or a lambda)
/// appears in a body.
struct Scan<'pr> {
    /// (receiver, `.` range) of each send.
    sends: Vec<(Option<Node<'pr>>, Option<(usize, usize)>)>,
    block: bool,
}

impl<'pr> Visit<'pr> for Scan<'pr> {
    fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
        if let Some(b) = c.block().and_then(|b| b.as_block_node()) {
            let numbered = b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some());
            if !numbered {
                self.block = true;
            }
        }
        if !is_csend(c) {
            self.sends.push((c.receiver(), c.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()))));
        }
        ruby_prism::visit_call_node(self, c);
    }
    // `a.b ||= c`, `a.b += c`, `a[i] ||= c`: parser's op-assign holds a send `a.b` / `a[i]`.
    fn visit_call_or_write_node(&mut self, n: &ruby_prism::CallOrWriteNode<'pr>) {
        if !n.is_safe_navigation() {
            self.sends.push((n.receiver(), n.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()))));
        }
        ruby_prism::visit_call_or_write_node(self, n);
    }
    fn visit_call_and_write_node(&mut self, n: &ruby_prism::CallAndWriteNode<'pr>) {
        if !n.is_safe_navigation() {
            self.sends.push((n.receiver(), n.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()))));
        }
        ruby_prism::visit_call_and_write_node(self, n);
    }
    fn visit_call_operator_write_node(&mut self, n: &ruby_prism::CallOperatorWriteNode<'pr>) {
        if !n.is_safe_navigation() {
            self.sends.push((n.receiver(), n.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()))));
        }
        ruby_prism::visit_call_operator_write_node(self, n);
    }
    fn visit_index_or_write_node(&mut self, n: &ruby_prism::IndexOrWriteNode<'pr>) {
        if !n.is_safe_navigation() {
            self.sends.push((n.receiver(), n.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()))));
        }
        ruby_prism::visit_index_or_write_node(self, n);
    }
    fn visit_index_and_write_node(&mut self, n: &ruby_prism::IndexAndWriteNode<'pr>) {
        if !n.is_safe_navigation() {
            self.sends.push((n.receiver(), n.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()))));
        }
        ruby_prism::visit_index_and_write_node(self, n);
    }
    fn visit_index_operator_write_node(&mut self, n: &ruby_prism::IndexOperatorWriteNode<'pr>) {
        if !n.is_safe_navigation() {
            self.sends.push((n.receiver(), n.call_operator_loc().map(|d| (d.start_offset(), d.end_offset()))));
        }
        ruby_prism::visit_index_operator_write_node(self, n);
    }
    fn visit_lambda_node(&mut self, l: &ruby_prism::LambdaNode<'pr>) {
        self.block = true;
        ruby_prism::visit_lambda_node(self, l);
    }
}

impl Cop for RedundantReceiverInWithOptions {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        if method_name(&call) != "with_options" {
            return;
        }
        let Some(body) = block.body() else { return };
        let mut scan = Scan { sends: Vec::new(), block: false };
        scan.visit(&body);
        if scan.block {
            return;
        }
        let params = block.parameters();
        let numbered = params.as_ref().and_then(|p| p.as_numbered_parameters_node()).is_some();
        let it = params.as_ref().and_then(|p| p.as_it_parameters_node()).is_some();
        let redundant = if numbered || it {
            // `n.receiver.lvar_type?` raises for a receiver-less send.
            if scan.sends.iter().any(|n| n.0.is_none()) {
                return;
            }
            scan.sends.iter().all(|n| {
                let r = n.0.as_ref().unwrap();
                if numbered {
                    r.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == b"_1")
                } else {
                    r.as_it_local_variable_read_node().is_some()
                }
            })
        } else {
            // node.first_argument: the first block parameter; same_value? compares its name with
            // the receiver's first child (an lvar's or ivar's name).
            let Some(first) = params
                .as_ref()
                .and_then(|p| p.as_block_parameters_node())
                .and_then(|bp| bp.parameters())
                .and_then(|ps| parameter_nodes(&ps).into_iter().next())
            else {
                return;
            };
            let name: Vec<u8> = if let Some(r) = first.as_required_parameter_node() {
                r.name().as_slice().to_vec()
            } else if let Some(o) = first.as_optional_parameter_node() {
                o.name().as_slice().to_vec()
            } else if let Some(k) = first.as_required_keyword_parameter_node() {
                k.name().as_slice().to_vec()
            } else if let Some(k) = first.as_optional_keyword_parameter_node() {
                k.name().as_slice().to_vec()
            } else {
                return;
            };
            scan.sends.iter().all(|n| {
                n.0.as_ref().is_some_and(|r| r.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name.as_slice()))
            })
        };
        if !redundant {
            return;
        }
        // block_argument_range: the `|...|` and the spaces before it (plain blocks only).
        let arg_range = if numbered || it {
            None
        } else {
            params.as_ref().map(|p| {
                let (mut s, e) = (p.location().start_offset(), p.location().end_offset());
                let b = cx.src.text.as_bytes();
                while s > 0 && b[s - 1] == b' ' {
                    s -= 1;
                }
                (s, e)
            })
        };
        for (r, dot) in &scan.sends {
            let r = r.as_ref().unwrap();
            let (s, e) = (r.location().start_offset(), r.location().end_offset());
            let dot = *dot;
            cx.add_fix(s, e, "Redundant receiver in `with_options`.", |c| {
                c.remove(s, e);
                if let Some((a, b)) = dot {
                    c.remove(a, b);
                }
                if let Some((a, b)) = arg_range {
                    c.remove(a, b);
                }
            });
        }
    }
}
