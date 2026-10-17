use crate::prelude::*;

pub const NAME: &str = "RSpec/EmptyMetadata";
pub static COP: EmptyMetadata = EmptyMetadata;
pub struct EmptyMetadata;

impl Cop for EmptyMetadata {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        for (_, list) in helpers::rspec::metadata_lists(&l, node) {
            let (_, hash) = helpers::rspec::split_metadata(&list);
            // hash&.pairs&.empty? and no kwsplat: an empty `{}`
            let Some(hash) = hash else { continue };
            if helpers::rails::hash_elements(hash).is_none_or(|e| !e.is_empty()) {
                continue;
            }
            let (hs, he) = helpers::rspec::node_range(hash);
            let (s, e) = helpers::rails::left_space_and_comma(cx.src.text, hs, he);
            cx.add_fix(hs, he, "Avoid empty metadata hash.", |c| c.remove(s, e));
        }
    }
}
