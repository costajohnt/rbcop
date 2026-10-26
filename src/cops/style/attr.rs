use crate::prelude::*;

pub const NAME: &str = "Style/Attr";
pub static COP: Attr = Attr;
pub struct Attr;

/// A parser `block` (not numblock/itblock): a call with a literal block, or a `->` lambda.
fn block_params_plain(params: Option<Node>) -> bool {
    params.is_none_or(|p| p.as_numbered_parameters_node().is_none() && p.as_it_parameters_node().is_none())
}

struct AttrDef(bool);

impl<'pr> ruby_prism::Visit<'pr> for AttrDef {
    fn visit_def_node(&mut self, node: &ruby_prism::DefNode<'pr>) {
        if node.receiver().is_none() && node.name().as_slice() == b"attr" {
            self.0 = true;
        }
        ruby_prism::visit_def_node(self, node);
    }
}

fn define_attr_method(node: &Node) -> bool {
    let mut v = AttrDef(false);
    ruby_prism::Visit::visit(&mut v, node);
    v.0
}

impl Cop for Attr {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "attr" {
            return;
        }
        let args = send_args(&call);
        if args.is_empty() {
            return;
        }
        // `attr :x do ... end`: its own block is the nearest block ancestor of the send.
        if call.block().and_then(|b| b.as_block_node()).is_some_and(|b| block_params_plain(b.parameters())) {
            return;
        }
        // allowed_context?: the nearest class or block ancestor.
        for anc in cx.ancestors.iter().rev() {
            match anc {
                Node::ClassNode { .. } => {
                    if define_attr_method(anc) {
                        return;
                    }
                    break;
                }
                Node::CallNode { .. } => {
                    let c = anc.as_call_node().unwrap();
                    let Some(b) = c.block().and_then(|b| b.as_block_node()) else { continue };
                    if !block_params_plain(b.parameters()) {
                        continue;
                    }
                    let class_eval = matches!(method_name(&c), "class_eval" | "module_eval") && !is_csend(&c);
                    if !class_eval || define_attr_method(anc) {
                        return;
                    }
                    break;
                }
                Node::LambdaNode { .. }
                    if block_params_plain(anc.as_lambda_node().unwrap().parameters()) => {
                        return;
                    }
                _ => {}
            }
        }
        let bool_node = |n: &Node| matches!(n, Node::TrueNode { .. } | Node::FalseNode { .. });
        let last = args.last().unwrap();
        let replacement = if last.as_true_node().is_some() {
            "attr_accessor"
        } else {
            "attr_reader"
        };
        let remove = args.get(1).filter(|s| bool_node(s)).map(|s| (args[0].location().end_offset(), s.location().end_offset()));
        let sel = call.message_loc().unwrap();
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, format!("Do not use `attr`. Use `{replacement}` instead."), |c| {
            c.replace(s, e, replacement);
            if let Some((a, b)) = remove {
                c.remove(a, b);
            }
        });
    }
}
