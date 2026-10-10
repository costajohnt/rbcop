use crate::prelude::*;

pub const NAME: &str = "Style/HashAsLastArrayItem";
pub static COP: HashAsLastArrayItem = HashAsLastArrayItem;
pub struct HashAsLastArrayItem;

fn is_hash(node: &Node) -> bool {
    node.as_hash_node().is_some() || node.as_keyword_hash_node().is_some()
}

impl Cop for HashAsLastArrayItem {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE, kind::KEYWORD_HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (braced, first) = match node.as_hash_node() {
            Some(h) => (true, h.elements().iter().next()),
            None => (false, node.as_keyword_hash_node().unwrap().elements().iter().next()),
        };
        if first.is_some_and(|f| f.as_assoc_splat_node().is_some()) {
            return;
        }
        let Some(array) = cx.ancestors.last().and_then(|p| p.as_array_node()) else { return };
        let braces_style = cx.cfg.str("EnforcedStyle") == Some("braces");
        let values: Vec<Node> = array.elements().iter().collect();
        if values.iter().all(|v| v.as_hash_node().map_or(v.as_keyword_hash_node().is_some() && !braces_style, |_| braces_style)) {
            return;
        }
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        if values.len() >= 2 && is_hash(&values[values.len() - 2]) {
            return;
        }
        if values.last().is_none_or(|l| l.location().start_offset() != s || l.location().end_offset() != e) {
            return;
        }
        if array.opening_loc().is_none_or(|o| o.as_slice() != b"[") {
            return;
        }
        let src = cx.src;
        if braces_style {
            if braced {
                return;
            }
            let array_start = array.location().start_offset();
            let single = src.line_of(s) == src.line_of(e) || src.line_of(s) == src.line_of(array_start);
            let indent = " ".repeat(src.column(s));
            cx.add_fix(s, e, "Wrap hash in `{` and `}`.", |c| {
                if single {
                    c.wrap(s, e, "{", "}");
                } else {
                    c.wrap(s, e, &format!("{{\n{indent}"), &format!("\n{indent}}}"));
                }
            });
        } else {
            let h = node.as_hash_node();
            let Some(h) = h.filter(|h| h.elements().iter().next().is_some()) else { return };
            let (open, close) = (h.opening_loc(), h.closing_loc());
            // remove_last_element_trailing_comma on the array's last child (this hash).
            let (_, after) = helpers::range_help::range_with_surrounding_space(
                src.text,
                s,
                e,
                helpers::range_help::Surrounding { side: helpers::range_help::Side::Right, ..Default::default() },
            );
            let comma = src.text.as_bytes().get(after) == Some(&b',');
            cx.add_fix(s, e, "Omit the braces around the hash.", |c| {
                if comma {
                    c.remove(after, after + 1);
                }
                c.remove(open.start_offset(), open.end_offset());
                c.remove(close.start_offset(), close.end_offset());
            });
        }
    }
}
