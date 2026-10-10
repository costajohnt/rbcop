use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateHashKey";
pub static COP: DuplicateHashKey = DuplicateHashKey;
pub struct DuplicateHashKey;

impl Cop for DuplicateHashKey {
    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE, kind::KEYWORD_HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        use helpers::literal_node::{key, recursive_basic_literal};
        let elements: Vec<Node> = match node.as_hash_node() {
            Some(h) => h.elements().iter().collect(),
            None => node.as_keyword_hash_node().unwrap().elements().iter().collect(),
        };
        let keys = elements.iter().filter_map(|e| e.as_assoc_node()).map(|p| p.key()).filter(|k| {
            recursive_basic_literal(k) || matches!(k, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. })
        });
        // Duplication#consecutive_duplicates: every occurrence of a key after its first.
        let mut seen = std::collections::HashSet::new();
        for k in keys {
            if !seen.insert(key(&k)) {
                let (s, e) = helpers::literal_node::expression_range(&k);
                cx.add(s, e, "Duplicated key in hash literal.");
            }
        }
    }
}
