use crate::prelude::*;
use helpers::range_help::{Side, Surrounding, range_with_surrounding_space};

pub const NAME: &str = "RSpec/MetadataStyle";
pub static COP: MetadataStyle = MetadataStyle;
pub struct MetadataStyle;

static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[\p{L}_][\p{L}\p{N}_]*[?!]?\z").unwrap());

impl Cop for MetadataStyle {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("symbol").to_string();
        let msg = format!("Use {style} style for metadata.");
        let src = cx.src;
        let text = src.text;
        for (owner, list) in helpers::rspec::metadata_lists(&l, node) {
            let (symbols, hash) = helpers::rspec::split_metadata(&list);
            let all = send_args(&owner);
            if style == "hash" {
                // match_ambiguous_trailing_metadata?: the send's last argument is no hash or symbol
                let ambiguous = all
                    .last()
                    .is_some_and(|last| !(helpers::rails::hash_elements(last).is_some() || helpers::rspec::is_sym(cx, last)));
                // extract_metadata_hash: (send _ _ _ ... $hash)
                let last_hash = all.last().filter(|l| all.len() > 1 && helpers::rails::hash_elements(l).is_some());
                let syms: Vec<&Node> = symbols.iter().filter(|s| helpers::rspec::is_sym(cx, s)).collect();
                for symbol in syms {
                    let (s, e) = helpers::rspec::node_range(symbol);
                    if ambiguous {
                        cx.add_fix(s, e, msg.clone(), |_| {});
                        continue;
                    }
                    let value = helpers::rails::sym_value(symbol).unwrap_or_default();
                    let pair = format!("{value}: true");
                    // remove_symbol
                    let (rs, re) = helpers::rails::left_space_and_comma(text, s, e);
                    // insert_pair
                    let insert: (usize, usize, String, bool) = match last_hash {
                        None => match owner.closing_loc() {
                            Some(c) => (c.start_offset(), c.end_offset(), format!(", {pair}"), false),
                            None => {
                                let end = send_range(&owner).1;
                                (end, end, format!(", {pair}"), false)
                            }
                        },
                        Some(h) => {
                            let elems = helpers::rails::hash_elements(h).unwrap();
                            if elems.iter().any(|e| e.as_assoc_node().is_some()) {
                                let (ls, le) = helpers::rspec::node_range(elems.last().unwrap());
                                (ls, le, format!(", {pair}"), true)
                            } else if let Some(hn) = h.as_hash_node() {
                                let open = hn.opening_loc();
                                (open.start_offset(), open.end_offset(), format!(" {pair} "), true)
                            } else {
                                // `loc.begin` of a hash without braces is nil: RuboCop raises
                                continue;
                            }
                        }
                    };
                    // The symbol's text becomes a `name:` label: only a plain one is written.
                    if !LABEL.is_match(&value) {
                        cx.add_withheld_fix(s, e, msg.clone());
                        continue;
                    }
                    cx.add_fix(s, e, msg.clone(), |c| {
                        c.remove(rs, re);
                        let (is, ie, ref t, after) = insert;
                        if after {
                            c.insert_after(is, ie, t);
                        } else {
                            c.insert_before(is, ie, t);
                        }
                    });
                }
            } else if style == "symbol" {
                let Some(hash) = hash else { continue };
                let elems = helpers::rails::hash_elements(hash).unwrap_or_default();
                let braces = hash.as_hash_node().is_some();
                // the argument before the hash: the symbol goes after it
                let hs = hash.location().start_offset();
                let Some(left) = all.iter().rev().find(|a| a.location().end_offset() <= hs) else { continue };
                let (_, left_end) = helpers::rspec::node_range(left);
                let left_start = left.location().start_offset();
                for (i, el) in elems.iter().enumerate() {
                    let Some(pair) = el.as_assoc_node() else { continue };
                    // (pair sym true)
                    if !(helpers::rspec::is_sym(cx, &pair.key()) && pair.value().as_true_node().is_some()) {
                        continue;
                    }
                    let (s, e) = helpers::rspec::node_range(el);
                    let remove = if !braces || i > 0 {
                        helpers::rails::left_space_and_comma(text, s, e)
                    } else if i + 1 < elems.len() {
                        let (cs, ce) = helpers::rails::range_with_surrounding_comma(text, s, e, false, true);
                        range_with_surrounding_space(text, cs, ce, Surrounding { side: Side::Right, ..Default::default() })
                    } else {
                        (s, e)
                    };
                    let key = pair.key();
                    let sym = helpers::ruby_string::symbol_inspect(key.as_symbol_node().unwrap().unescaped());
                    cx.add_fix(s, e, msg.clone(), |c| {
                        c.remove(remove.0, remove.1);
                        c.insert_after(left_start, left_end, &format!(", {sym}"));
                    });
                }
            }
        }
    }
}
