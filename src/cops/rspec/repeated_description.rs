use crate::prelude::*;

pub const NAME: &str = "RSpec/RepeatedDescription";
pub static COP: RepeatedDescription = RepeatedDescription;
pub struct RepeatedDescription;

/// (signature, offense ranges)
type Groups = Vec<(String, Vec<(usize, usize)>)>;

impl Cop for RepeatedDescription {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        if helpers::rspec::plain_block_call(node).is_none() || !helpers::rspec::example_group(&l, node) {
            return;
        }
        let src = cx.src;
        let examples = helpers::rspec::find_all_in_scope(&l, node, helpers::rspec::example);
        // repeated_descriptions: group by [metadata, doc_string]; a signature with neither is skipped
        let mut groups: Groups = Vec::new();
        let mut its_groups: Groups = Vec::new();
        for ex in &examples {
            let call = ex.as_call_node().unwrap();
            let (doc, meta) = helpers::rspec::example_parts(&call);
            if method_name(&call) == "its" {
                // its_signature: [doc_string, example]; the example (its node) is never nil
                let key = format!("{}|{}", helpers::rspec::skey(src, doc.as_ref()), helpers::rspec::skey(src, Some(ex)));
                push(&mut its_groups, key, helpers::rspec::node_range(ex));
            } else {
                if doc.is_none() && meta.is_none() {
                    continue;
                }
                let key = format!(
                    "{}|{}",
                    meta.map_or("nil".to_string(), |m| helpers::rspec::skeys(src, &m)),
                    helpers::rspec::skey(src, doc.as_ref())
                );
                push(&mut groups, key, send_range(&call));
            }
        }
        for (_, group) in groups.into_iter().chain(its_groups) {
            if group.len() < 2 {
                continue;
            }
            for (s, e) in group {
                cx.add(s, e, "Don't repeat descriptions within an example group.");
            }
        }
    }
}

fn push(groups: &mut Groups, key: String, range: (usize, usize)) {
    match groups.iter_mut().find(|(k, _)| *k == key) {
        Some((_, v)) => v.push(range),
        None => groups.push((key, vec![range])),
    }
}
