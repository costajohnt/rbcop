use crate::prelude::*;

pub const NAME: &str = "Rails/DotSeparatedKeys";
pub static COP: DotSeparatedKeys = DotSeparatedKeys;
pub struct DotSeparatedKeys;

/// `basic_literal?` scope values and their `value.to_s`; None when a scope is not convertible
/// (or its `value` would raise in the corrector).
fn literal_value(n: &Node) -> Option<Option<String>> {
    if let Some(s) = helpers::rails::str_or_sym(n) {
        return Some(Some(s));
    }
    if n.as_integer_node().is_some() {
        return Some(int_literal_value(n).map(|i| i.to_string()));
    }
    if let Some(f) = n.as_float_node() {
        return Some(Some(helpers::ruby_string::float_to_s(f.value())));
    }
    // true/false/nil/complex/rational are basic literals too, but `value` raises (or is not modelled).
    let basic = matches!(
        n,
        Node::TrueNode { .. } | Node::FalseNode { .. } | Node::NilNode { .. } | Node::ImaginaryNode { .. } | Node::RationalNode { .. }
    );
    basic.then_some(None)
}

impl Cop for DotSeparatedKeys {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send {nil? (const {nil? cbase} :I18n)} {:translate :t} ${sym_type? str_type?}
        //   (hash <$(pair (sym :scope) ${array_type? sym_type?}) ...>))
        let Some(call) = helpers::rails::send(node) else { return };
        if !matches!(method_name(&call), "translate" | "t")
            || !call.receiver().is_none_or(|r| helpers::rails::is_const(&r, "I18n"))
        {
            return;
        }
        let args = send_args(&call);
        let [key, hash] = args.as_slice() else { return };
        let Some(key_value) = helpers::rails::str_or_sym(key) else { return };
        let Some(pair) = helpers::rails::hash_pair(hash, "scope") else { return };
        let value = pair.value();
        let scopes: Vec<Node> = if let Some(a) = value.as_array_node() {
            a.elements().iter().collect()
        } else if value.as_symbol_node().is_some() {
            vec![value]
        } else {
            return;
        };
        let mut parts = Vec::new();
        for s in &scopes {
            match literal_value(s) {
                None => return,
                Some(None) => return,
                Some(Some(v)) => parts.push(v),
            }
        }
        let mut new_key = format!("'{}.{key_value}'", parts.join("."));
        // squeeze('.')
        while new_key.contains("..") {
            new_key = new_key.replace("..", ".");
        }
        let src = cx.src;
        let (ps, pe) = (pair.location().start_offset(), pair.location().end_offset());
        let (rs, re) = helpers::rails::left_space_and_comma(&src.text, ps, pe);
        let (ks, ke) = (key.location().start_offset(), key.location().end_offset());
        cx.add_fix(ps, pe, "Use the dot-separated keys instead of specifying the `:scope` option.", |c| {
            c.remove(rs, re);
            c.replace(ks, ke, &new_key);
        });
    }
}
