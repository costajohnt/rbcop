use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/MixinGrouping";
pub static COP: MixinGrouping = MixinGrouping;
pub struct MixinGrouping;

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A direct-child `send` macro (receiver nil, no literal block).
fn macro_send<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    let c = n.as_call_node()?;
    (c.receiver().is_none() && !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none())).then_some(c)
}

impl Cop for MixinGrouping {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let body = match node.as_class_node() {
            Some(c) => c.body(),
            None => node.as_module_node().unwrap().body(),
        };
        // a 2+ statement body is a parser `begin`; otherwise the class node's own children
        let Some(stmts) = body.as_ref().and_then(|b| b.as_statements_node()) else { return };
        let children: Vec<Node> = stmts.body().iter().collect();
        let sends: Vec<(CallNode, (usize, usize))> =
            children.iter().filter_map(|n| macro_send(n).map(|c| (c, range(n)))).collect();
        let grouped = cx.cfg.str("EnforcedStyle") == Some("grouped");
        let src = cx.src;
        for (call, me) in &sends {
            let name = method_name(call);
            let args = send_args(call);
            if !matches!(name, "extend" | "include" | "prepend") || args.is_empty() {
                continue;
            }
            if grouped {
                let mixins: Vec<&(CallNode, (usize, usize))> = sends.iter().filter(|(c, _)| method_name(c) == name).collect();
                if mixins.len() == 1 {
                    continue;
                }
                let same = |a: (usize, usize), b: (usize, usize)| helpers::node_equality::same(src, a, b);
                let (mut rs, re) = *me;
                let correction = if same(*me, mixins[0].1) {
                    let names: Vec<&str> =
                        mixins.iter().rev().flat_map(|(c, _)| send_args(c).iter().map(node_text).collect::<Vec<_>>()).collect();
                    format!("{name} {}", names.join(", "))
                } else {
                    let prev = mixins.windows(2).find(|w| same(w[1].1, *me)).map(|w| w[0].1);
                    if let Some((_, pe)) = prev
                        && !src.text[pe..rs].chars().any(|c| !c.is_ascii_whitespace()) {
                            rs = pe;
                        }
                    String::new()
                };
                cx.add_fix(me.0, me.1, format!("Put `{name}` mixins in a single statement."), |c| {
                    c.replace(rs, re, &correction)
                });
            } else {
                if args.len() == 1 {
                    continue;
                }
                let indent = " ".repeat(src.column(me.0));
                let lines: Vec<String> = args
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, a)| format!("{}{name} {}", if i == 0 { "" } else { &indent }, node_text(a)))
                    .collect();
                let correction = lines.join("\n");
                let (s, e) = *me;
                cx.add_fix(s, e, format!("Put `{name}` mixins in separate statements."), |c| c.replace(s, e, &correction));
            }
        }
    }
}
