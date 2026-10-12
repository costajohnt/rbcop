use crate::prelude::*;

pub const NAME: &str = "Rails/RedundantAllowNil";
pub static COP: RedundantAllowNil = RedundantAllowNil;
pub struct RedundantAllowNil;

/// The child nodes find_allow_nil_and_allow_blank walks (parser `each_child_node` for the node
/// types a validation's options are made of).
// ponytail: only sends, hashes, pairs and arrays are descended; options nested in other node
// types (lambdas, conditionals) are not searched.
fn children<'pr>(n: &Node<'pr>) -> Vec<Node<'pr>> {
    if let Some(c) = n.as_call_node() {
        let mut v: Vec<Node> = c.receiver().into_iter().collect();
        v.extend(send_args(&c));
        return v;
    }
    if let Some(e) = helpers::rails::hash_elements(n) {
        return e;
    }
    if let Some(a) = n.as_assoc_node() {
        return vec![a.key(), a.value()];
    }
    if let Some(a) = n.as_array_node() {
        return a.elements().iter().collect();
    }
    Vec::new()
}

/// The first node (depth first) whose direct pair children include both `allow_nil` and
/// `allow_blank`: (its children, allow_nil index, allow_blank index).
fn find<'pr>(n: &Node<'pr>, src: &crate::source::Source) -> Option<(Vec<Node<'pr>>, usize, usize)> {
    let kids = children(n);
    let (mut nil, mut blank) = (None, None);
    for (i, k) in kids.iter().enumerate() {
        if let Some(p) = k.as_assoc_node() {
            let key = p.key();
            let (s, e) = if key.as_symbol_node().is_some() { helpers::rails::sym_range(&key) } else { (key.location().start_offset(), key.location().end_offset()) };
            match src.slice(s, e) {
                "allow_nil" => nil = Some(i),
                "allow_blank" => blank = Some(i),
                _ => {}
            }
        }
        if let (Some(a), Some(b)) = (nil, blank) {
            return Some((kids, a, b));
        }
        if let Some(found) = find(k, src) {
            return Some(found);
        }
    }
    None
}

impl Cop for RedundantAllowNil {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "validates" {
            return;
        }
        let Some((kids, ni, bi)) = find(node, cx.src) else { return };
        let (nil, blank) = (kids[ni].as_assoc_node().unwrap(), kids[bi].as_assoc_node().unwrap());
        let (nv, bv) = (nil.value(), blank.value());
        let msg = if helpers::literal_node::parser_type(&nv) == helpers::literal_node::parser_type(&bv) {
            "`allow_nil` is redundant when `allow_blank` has the same value."
        } else if nv.as_false_node().is_some() && bv.as_true_node().is_some() {
            "`allow_nil: false` is redundant when `allow_blank` is true."
        } else {
            return;
        };
        let range = |n: &Node| (n.location().start_offset(), n.location().end_offset());
        let (s, e) = range(&kids[ni]);
        let removal = if let Some(next) = kids.get(ni + 1) {
            (s, range(next).0)
        } else if ni > 0 {
            (range(&kids[ni - 1]).1, e)
        } else {
            (s, e)
        };
        cx.add_fix(s, e, msg, |c| c.remove(removal.0, removal.1));
    }
}
