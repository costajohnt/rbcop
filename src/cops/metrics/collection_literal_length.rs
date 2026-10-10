use crate::prelude::*;

pub const NAME: &str = "Metrics/CollectionLiteralLength";
pub static COP: CollectionLiteralLength = CollectionLiteralLength;
pub struct CollectionLiteralLength;

const MSG: &str = "Avoid hard coding large quantities of data in code. Prefer reading the data from an external source.";

// ponytail: parser's `rescue A, B` exception list is an `array` too; Prism has no ArrayNode there.
// Only matters with Max below the number of rescued classes.
impl Cop for CollectionLiteralLength {
    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE, kind::HASH_NODE, kind::KEYWORD_HASH_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(max) = cx.cfg.int("Max") else { return };
        let len = match node {
            Node::ArrayNode { .. } => node.as_array_node().unwrap().elements().iter().count(),
            Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().count(),
            Node::KeywordHashNode { .. } => node.as_keyword_hash_node().unwrap().elements().iter().count(),
            _ => {
                let call = node.as_call_node().unwrap();
                // RESTRICT_ON_SEND without on_csend
                if method_name(&call) != "[]" || is_csend(&call) || !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Set")) {
                    return;
                }
                send_args(&call).len()
            }
        };
        if len as i64 >= max {
            let (s, e) = match node.as_call_node() {
                Some(c) => send_range(&c),
                None => (node.location().start_offset(), node.location().end_offset()),
            };
            cx.add(s, e, MSG);
        }
    }
}
