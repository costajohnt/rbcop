use crate::prelude::*;

pub const NAME: &str = "Rails/Validation";
pub static COP: Validation = Validation;
pub struct Validation;

const TYPES: &[&str] = &[
    "acceptance", "comparison", "confirmation", "exclusion", "format", "inclusion", "length", "numericality", "presence",
    "absence", "size", "uniqueness",
];

/// rubocop-ast `literal?`
fn literal(n: &Node) -> bool {
    matches!(
        n,
        Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::XStringNode { .. }
            | Node::InterpolatedXStringNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::InterpolatedRegularExpressionNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::KeywordHashNode { .. }
            | Node::RangeNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::RationalNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
    )
}

/// correct_validate_type_for_array: the attributes (`:x` for a %-literal's words).
fn array_attrs(a: &ruby_prism::ArrayNode) -> String {
    let percent = a.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%"));
    a.elements()
        .iter()
        .map(|e| if percent { format!(":{}", node_text(&e)) } else { node_text(&e).to_string() })
        .collect::<Vec<_>>()
        .join(", ")
}

impl Cop for Validation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if call.receiver().is_some() {
            return;
        }
        let name = method_name(&call);
        let Some(ty) = name.strip_prefix("validates_").and_then(|r| r.strip_suffix("_of")) else { return };
        if !TYPES.contains(&ty) {
            return;
        }
        let args = send_args(&call);
        let Some(last) = args.last() else { return };
        let frozen = helpers::rails::send_node(last).filter(|c| method_name(c) == "freeze");
        // `return` in the corrector block: no offense at all.
        if !literal(last) && last.as_splat_node().is_none() && frozen.is_none() {
            return;
        }
        let validate_type = if ty == "size" { "length" } else { ty };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (ls, le) = (last.location().start_offset(), last.location().end_offset());
        let edit: (usize, usize, String) = if let Some(h) = helpers::rails::hash_elements(last).map(|_| last) {
            let braced = if h.as_hash_node().is_some() { node_text(h).to_string() } else { format!("{{ {} }}", node_text(h)) };
            (ls, le, format!("{validate_type}: {braced}"))
        } else if let Some(a) = last.as_array_node() {
            (ls, le, format!("{}, {validate_type}: true", array_attrs(&a)))
        } else if let Some(f) = frozen {
            // `x.freeze`: only an array receiver can be corrected (`percent_literal?` raises otherwise).
            let Some(a) = f.receiver().and_then(|r| r.as_array_node()) else { return };
            (ls, le, format!("{}, {validate_type}: true", array_attrs(&a)))
        } else {
            (le, le, format!(", {validate_type}: true"))
        };
        let msg = format!("Prefer the new style validations `validates :column, {ty}: value` over `{name}`.");
        cx.add_fix(ss, se, msg, |c| {
            c.replace(ss, se, "validates");
            if edit.0 == edit.1 {
                c.insert_after(ls, le, &edit.2);
            } else {
                c.replace(edit.0, edit.1, &edit.2);
            }
        });
    }
}
