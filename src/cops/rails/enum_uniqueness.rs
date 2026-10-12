use crate::prelude::*;

pub const NAME: &str = "Rails/EnumUniqueness";
pub static COP: EnumUniqueness = EnumUniqueness;
pub struct EnumUniqueness;

fn enum_name(key: &Node) -> String {
    helpers::rails::str_or_sym(key).unwrap_or_else(|| node_text(key).to_string())
}

/// `args.values` of an array (elements) or hash (pair values).
fn values<'pr>(n: &Node<'pr>) -> Vec<Node<'pr>> {
    if let Some(a) = n.as_array_node() {
        return a.elements().iter().collect();
    }
    helpers::rails::hash_pairs(n).iter().map(|p| p.value()).collect()
}

fn check(cx: &mut Cx, key: &Node, collection: &Node) {
    // Duplication#consecutive_duplicates: every structurally equal item after the first.
    let items = values(collection);
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let shape = helpers::rails::node_key(cx.src, it);
        match groups.iter_mut().find(|(k, _)| *k == shape) {
            Some((_, v)) => v.push(i),
            None => groups.push((shape, vec![i])),
        }
    }
    let name = enum_name(key);
    for (_, idx) in groups {
        for &i in idx.iter().skip(1) {
            let it = &items[i];
            let msg = format!("Duplicate value `{}` found in `{name}` enum declaration.", node_text(it));
            cx.add(it.location().start_offset(), it.location().end_offset(), msg);
        }
    }
}

impl Cop for EnumUniqueness {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "enum") {
            return;
        }
        let args = send_args(&call);
        let coll = |n: &Node| n.as_array_node().is_some() || helpers::rails::hash_elements(n).is_some();
        // (send nil? :enum $_ ${array hash} ...)
        if args.len() >= 2 && coll(&args[1]) {
            check(cx, &args[0], &args[1]);
        }
        // (send nil? :enum (hash $...)): each (pair $_ ${array hash})
        if let [hash] = args.as_slice()
            && helpers::rails::hash_elements(hash).is_some()
        {
            for p in helpers::rails::hash_pairs(hash) {
                if coll(&p.value()) {
                    check(cx, &p.key(), &p.value());
                }
            }
        }
    }
}
