use crate::prelude::*;

pub const NAME: &str = "Metrics/CollectionLiteralLength";
pub static COP: CollectionLiteralLength = CollectionLiteralLength;
pub struct CollectionLiteralLength;

const MSG: &str = "Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.";

impl Cop for CollectionLiteralLength {
    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE, kind::HASH_NODE, kind::KEYWORD_HASH_NODE, kind::CALL_NODE, kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let len = match node {
            Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter().count(),
            Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().count(),
            Node::KeywordHashNode { .. } => node.as_keyword_hash_node().unwrap().elements().iter().count(),
            // parser's `rescue A, B` exception list is an `array`
            Node::RescueNode { .. } => match node.as_rescue_node().unwrap().exceptions().iter().count() {
                0 => return,
                n => n,
            },
            _ => {
                let call = node.as_call_node().unwrap();
                // RESTRICT_ON_SEND without on_csend
                if method_name(&call) != "[]" || is_csend(&call) || !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Set")) {
                    return;
                }
                send_args(&call).len()
            }
        };
        let (s, e) = match node {
            Node::CallNode { .. } => send_range(&node.as_call_node().unwrap()),
            Node::RescueNode { .. } => {
                let ex: Vec<Node> = node.as_rescue_node().unwrap().exceptions().iter().collect();
                (ex[0].location().start_offset(), ex[ex.len() - 1].location().end_offset())
            }
            _ => (node.location().start_offset(), node.location().end_offset()),
        };
        // collection_threshold: `cop_config.fetch('Max', Float::INFINITY)`, read for each
        // collection (a String one raises there; the rescue list's `array` starts at its first class)
        let (cop, src) = (cx.cop, cx.src);
        let at = if node.as_rescue_node().is_some() { Some(s) } else { crate::runner::visiting() };
        crate::runner::own_callback(cop, src, at, || {
            let Some(max) = cx.cfg.num("Max") else { return };
            if len as f64 >= max.f64() {
                cx.add(s, e, MSG);
            }
        });
    }
}
