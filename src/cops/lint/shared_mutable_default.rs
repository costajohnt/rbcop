use crate::prelude::*;
use helpers::parser_parent::is_top_const;

pub const NAME: &str = "Lint/SharedMutableDefault";
pub static COP: SharedMutableDefault = SharedMutableDefault;
pub struct SharedMutableDefault;

/// Pairs of a parser `hash` (braced or keyword hash).
fn hash_pairs<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    if let Some(h) = node.as_hash_node() {
        return Some(h.elements().iter().collect());
    }
    node.as_keyword_hash_node().map(|h| h.elements().iter().collect())
}

/// `{array hash (send (const {nil? cbase} {:Array :Hash}) :new)}`
fn mutable(node: &Node) -> bool {
    if node.as_array_node().is_some() || hash_pairs(node).is_some() {
        return true;
    }
    node.as_call_node().is_some_and(|c| {
        !is_csend(&c)
            && method_name(&c) == "new"
            && c.arguments().is_none()
            && c.block().is_none()
            && c.receiver().is_some_and(|r| is_top_const(&r, "Array") || is_top_const(&r, "Hash"))
    })
}

/// `(hash (pair (sym :capacity) _))`
fn capacity(node: &Node) -> bool {
    let Some(pairs) = hash_pairs(node) else { return false };
    let [pair] = pairs.as_slice() else { return false };
    pair.as_assoc_node()
        .and_then(|p| p.key().as_symbol_node())
        .is_some_and(|s| s.unescaped() == b"capacity")
}

impl Cop for SharedMutableDefault {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "new" || is_csend(&call) || !call.receiver().is_some_and(|r| is_top_const(&r, "Hash")) {
            return;
        }
        let args = send_args(&call);
        let hit = match args.as_slice() {
            [a] => mutable(a) && !capacity(a),
            [a, b] => mutable(a) && capacity(b),
            _ => false,
        };
        if hit {
            let (s, e) = send_range(&call);
            cx.add(
                s,
                e,
                "Do not create a Hash with a mutable default value as the default value can accidentally be changed.",
            );
        }
    }
}
