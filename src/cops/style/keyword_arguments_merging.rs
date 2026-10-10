use crate::prelude::*;

pub const NAME: &str = "Style/KeywordArgumentsMerging";
pub static COP: KeywordArgumentsMerging = KeywordArgumentsMerging;
pub struct KeywordArgumentsMerging;

impl Cop for KeywordArgumentsMerging {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ASSOC_SPLAT_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // kwsplat -> hash -> (ArgumentsNode) -> send
        let n = cx.ancestors.len();
        if n < 3 {
            return;
        }
        let hash = &cx.ancestors[n - 1];
        let elements: Vec<Node> = match hash {
            Node::KeywordHashNode { .. } => hash.as_keyword_hash_node().unwrap().elements().iter().collect(),
            Node::HashNode { .. } => hash.as_hash_node().unwrap().elements().iter().collect(),
            _ => return,
        };
        if cx.ancestors[n - 2].as_arguments_node().is_none() {
            return;
        }
        let Some(send) = cx.ancestors[n - 3].as_call_node() else { return };
        if is_csend(&send) {
            return;
        }
        // The hash is the last argument (a block-pass would follow it).
        let args = send_args(&send);
        if args.last().is_none_or(|a| a.location().start_offset() != hash.location().start_offset()) {
            return;
        }
        // `(hash (kwsplat $(send $_ :merge $...)) ...)`: the first element.
        let Some(first) = elements.first().and_then(|e| e.as_assoc_splat_node()) else { return };
        let Some(merge) = first.value().and_then(|v| v.as_call_node()) else { return };
        if is_csend(&merge) || method_name(&merge) != "merge" || merge.block().is_some_and(|b| b.as_block_node().is_some())
        {
            return;
        }
        let others = send_args(&merge);
        if others.iter().any(|o| o.as_block_argument_node().is_some()) {
            return;
        }
        // `hash_node.source` raises on a receiverless `merge`, losing the offense.
        let Some(receiver) = merge.receiver() else { return };
        let replacement = others
            .iter()
            .map(|o| match o {
                Node::HashNode { .. } => {
                    let t = node_text(o);
                    let mut chars = t.chars();
                    chars.next();
                    chars.next_back();
                    chars.as_str().to_string()
                }
                Node::KeywordHashNode { .. } => node_text(o).to_string(),
                _ => format!("**{}", node_text(o)),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let replacement = format!("**{}, {replacement}", node_text(&receiver));
        let (ms, me) = send_range(&merge);
        let l = node.location();
        let (ks, ke) = (l.start_offset(), l.end_offset());
        cx.add_fix(ms, me, "Provide additional arguments directly rather than using `merge`.", |c| {
            c.replace(ks, ke, &replacement)
        });
    }
}
