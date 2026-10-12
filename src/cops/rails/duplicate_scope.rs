use crate::prelude::*;

pub const NAME: &str = "Rails/DuplicateScope";
pub static COP: DuplicateScope = DuplicateScope;
pub struct DuplicateScope;

impl Cop for DuplicateScope {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        // (send nil? :scope _ $...): grouped by the arguments after the name (structurally).
        type Group = (Vec<String>, Vec<(usize, usize)>);
        let mut groups: Vec<Group> = Vec::new();
        for call in helpers::rails::class_send_nodes(class.body()) {
            if !helpers::rails::is_command(&call, "scope") {
                continue;
            }
            let args = send_args(&call);
            if args.is_empty() {
                continue;
            }
            let key: Vec<String> = args[1..].iter().map(|a| helpers::rails::node_key(cx.src, a)).collect();
            let range = send_range(&call);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, v)) => v.push(range),
                None => groups.push((key, vec![range])),
            }
        }
        for (_, ranges) in groups.into_iter().filter(|(_, v)| v.len() > 1) {
            for (s, e) in ranges {
                cx.add(s, e, "Multiple scopes share this same expression.");
            }
        }
    }
}
