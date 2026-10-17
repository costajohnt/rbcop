use crate::prelude::*;

pub const NAME: &str = "RSpec/SortMetadata";
pub static COP: SortMetadata = SortMetadata;
pub struct SortMetadata;

impl Cop for SortMetadata {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let src = cx.src;
        for (owner, list) in helpers::rspec::metadata_lists(&l, node) {
            let (args, hash) = helpers::rspec::split_metadata(&list);
            let pairs: Vec<ruby_prism::AssocNode> =
                hash.and_then(helpers::rails::hash_elements).unwrap_or_default().into_iter().filter_map(|p| p.as_assoc_node()).collect();
            // last_arg_could_be_a_hash?: the send's last argument is no hash, symbol or string
            let mut args: &[Node] = args;
            let all = send_args(&owner);
            if !args.is_empty()
                && all.last().is_some_and(|last| {
                    !(helpers::rails::hash_elements(last).is_some()
                        || helpers::rspec::is_sym(cx, last)
                        || matches!(
                            kind::of(last),
                            kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE | kind::X_STRING_NODE | kind::INTERPOLATED_X_STRING_NODE
                        ))
                })
            {
                args = &args[..args.len() - 1];
            }
            let n_syms = args.iter().rev().take_while(|a| helpers::rspec::is_sym(cx, a)).count();
            let symbols = &args[args.len() - n_syms..];
            let sym_key = |n: &Node| helpers::rails::sym_value(n).unwrap_or_default().to_lowercase();
            let mut sorted_syms: Vec<&Node> = symbols.iter().collect();
            sorted_syms.sort_by_key(|n| sym_key(n));
            let mut sorted_pairs: Vec<&ruby_prism::AssocNode> = pairs.iter().collect();
            sorted_pairs.sort_by_key(|p| node_text(&p.key()).to_lowercase());
            // sorted?: structural equality of the node lists
            let key = |n: &Node| helpers::rspec::skey(src, Some(n));
            let syms_sorted = symbols.iter().zip(&sorted_syms).all(|(a, b)| key(a) == key(b));
            let pairs_sorted = pairs.iter().zip(&sorted_pairs).all(|(a, b)| key(&a.as_node()) == key(&b.as_node()));
            if syms_sorted && pairs_sorted {
                continue;
            }
            let first = symbols.first().map(|s| s.location().start_offset()).or_else(|| pairs.first().map(|p| p.location().start_offset()));
            let last = pairs.last().map(|p| p.location().end_offset()).or_else(|| symbols.last().map(|s| s.location().end_offset()));
            let (Some(s), Some(e)) = (first, last) else { continue };
            let parts: Vec<&str> = sorted_syms
                .iter()
                .map(|n| node_text(n))
                .chain(sorted_pairs.iter().map(|p| src.slice(p.location().start_offset(), p.location().end_offset())))
                .collect();
            let replacement = parts.join(", ");
            cx.add_fix(s, e, "Sort metadata alphabetically.", |c| c.replace(s, e, &replacement));
        }
    }
}
