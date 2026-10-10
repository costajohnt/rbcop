use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/RedundantStructKeywordInit";
pub static COP: RedundantStructKeywordInit = RedundantStructKeywordInit;
pub struct RedundantStructKeywordInit;

/// `(pair (sym :keyword_init) _)`: the value, if `n` is such a pair.
fn keyword_init_value<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    let pair = n.as_assoc_node()?;
    let sym = pair.key();
    let sym = sym.as_symbol_node()?;
    (sym.unescaped() == b"keyword_init").then(|| pair.value())
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

impl Cop for RedundantStructKeywordInit {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.2, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "new" || !call.receiver().is_some_and(|r| is_top_const(&r, "Struct")) {
            return;
        }
        if call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        let args = call_args(&call);
        let Some(hash) = args.last() else { return };
        let elements: Vec<Node> = match hash {
            Node::HashNode { .. } => hash.as_hash_node().unwrap().elements().iter().collect(),
            Node::KeywordHashNode { .. } => hash.as_keyword_hash_node().unwrap().elements().iter().collect(),
            _ => return,
        };
        let values: Vec<Option<Node>> = elements.iter().map(keyword_init_value).collect();
        if values.iter().flatten().any(|v| v.as_false_node().is_some()) {
            return;
        }
        let pairs: Vec<usize> = (0..elements.len()).filter(|&i| elements[i].as_assoc_node().is_some()).collect();
        let all_keyword_init = pairs.iter().all(|&i| values[i].is_some());
        // the hash's left sibling among the send's children (the method name for the first argument)
        let preceding = (args.len() >= 2).then(|| range(&args[args.len() - 2]));
        for (i, value) in values.iter().enumerate() {
            let Some(value) = value else { continue };
            if value.as_true_node().is_none() && value.as_nil_node().is_none() {
                continue;
            }
            let pair = range(&elements[i]);
            let remove = if all_keyword_init {
                match preceding {
                    Some(p) => (p.1, pair.1),
                    None => pair,
                }
            } else if i > 0 {
                (range(&elements[i - 1]).1, pair.1)
            } else if let Some(right) = elements.get(1) {
                (pair.0, range(right).0)
            } else {
                pair
            };
            let msg = format!("Remove the redundant `keyword_init: {}`.", node_text(value));
            cx.add_fix(pair.0, pair.1, msg, |c| c.remove(remove.0, remove.1));
        }
    }
}
