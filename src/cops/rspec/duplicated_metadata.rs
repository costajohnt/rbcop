use crate::prelude::*;

pub const NAME: &str = "RSpec/DuplicatedMetadata";
pub static COP: DuplicatedMetadata = DuplicatedMetadata;
pub struct DuplicatedMetadata;

impl Cop for DuplicatedMetadata {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        for (owner, list) in helpers::rspec::metadata_lists(&l, node) {
            let (symbols, _) = helpers::rspec::split_metadata(&list);
            let args = send_args(&owner);
            for symbol in symbols {
                // left_siblings: the send's earlier arguments (the description included)
                let me = helpers::rspec::node_range(symbol);
                let dup = args
                    .iter()
                    .take_while(|a| helpers::rspec::node_range(a) != me)
                    .any(|a| helpers::node_equality::same(cx.src, helpers::rspec::node_range(a), me));
                if !dup {
                    continue;
                }
                let (s, e) = helpers::rails::left_space_and_comma(cx.src.text, me.0, me.1);
                cx.add_fix(me.0, me.1, "Avoid duplicated metadata.", |c| c.remove(s, e));
            }
        }
    }
}
