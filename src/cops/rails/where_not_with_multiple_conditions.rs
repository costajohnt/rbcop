use crate::prelude::*;

pub const NAME: &str = "Rails/WhereNotWithMultipleConditions";
pub static COP: WhereNotWithMultipleConditions = WhereNotWithMultipleConditions;
pub struct WhereNotWithMultipleConditions;

fn multiple(hash: &Node) -> bool {
    let elems = helpers::rails::hash_elements(hash).unwrap_or_default();
    let pairs: Vec<ruby_prism::AssocNode> = elems.iter().filter_map(|e| e.as_assoc_node()).collect();
    if pairs.len() >= 2 {
        return true;
    }
    match pairs.first() {
        Some(p) if helpers::rails::hash_elements(&p.value()).is_some() => multiple(&p.value()),
        _ => false,
    }
}

impl Cop for WhereNotWithMultipleConditions {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send _ :where) :not $...)
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "not" {
            return;
        }
        let Some(recv) = call.receiver().and_then(|r| helpers::rails::send_node(&r)) else { return };
        if method_name(&recv) != "where" || !send_args(&recv).is_empty() {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        if helpers::rails::hash_elements(first).is_none() || !multiple(first) {
            return;
        }
        let s = recv.message_loc().unwrap().start_offset();
        let (_, e) = send_range(&call);
        cx.add(s, e, "Use a SQL statement instead of `where.not` with multiple conditions.");
    }
}
