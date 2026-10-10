use crate::prelude::*;

pub const NAME: &str = "Lint/UselessConstantScoping";
pub static COP: UselessConstantScoping = UselessConstantScoping;
pub struct UselessConstantScoping;

/// `bare_access_modifier?` name of a sibling statement (macro scope is checked by the caller).
fn bare_access_modifier<'a>(node: &Node<'a>) -> Option<&'a str> {
    let call = node.as_call_node()?;
    if call.receiver().is_some() || call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    let name = method_name(&call);
    matches!(name, "public" | "protected" | "private" | "module_function").then_some(name)
}

/// `private_constant` arguments' `value.to_sym` (only plain symbols and strings have a value).
fn private_constant_names(node: &Node) -> Vec<Vec<u8>> {
    let Some(call) = node.as_call_node() else { return vec![] };
    if call.receiver().is_some() || method_name(&call) != "private_constant" || call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return vec![];
    }
    send_args(&call)
        .iter()
        .filter_map(|a| match kind::of(a) {
            kind::SYMBOL_NODE => Some(a.as_symbol_node().unwrap().unescaped().to_vec()),
            kind::STRING_NODE => Some(a.as_string_node().unwrap().unescaped().to_vec()),
            _ => None,
        })
        .collect()
}

impl Cop for UselessConstantScoping {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_WRITE_NODE, kind::CONSTANT_PATH_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // Only statements have siblings that can be access modifiers.
        let Some(stmts) = cx.parent().and_then(|p| p.as_statements_node()) else { return };
        let (name, value) = match node.as_constant_write_node() {
            Some(w) => (w.name().as_slice().to_vec(), w.value()),
            None => {
                let w = node.as_constant_path_write_node().unwrap();
                let Some(n) = w.target().name() else { return };
                (n.as_slice().to_vec(), w.value())
            }
        };
        let start = node.location().start_offset();
        let siblings: Vec<_> = stmts.body().iter().collect();
        let idx = siblings.iter().position(|s| s.location().start_offset() == start).unwrap();
        let in_macro_scope = helpers::macro_scope::in_macro_scope_of_child(cx.ancestors);
        let last_modifier = siblings[..idx].iter().rev().find_map(bare_access_modifier).filter(|_| in_macro_scope);
        if last_modifier != Some("private") {
            return;
        }
        if siblings[idx + 1..].iter().any(|s| private_constant_names(s).contains(&name)) {
            return;
        }
        if value.as_call_node().is_some_and(|c| helpers::macro_scope::class_constructor(&c)) {
            return;
        }
        cx.add(start, node.location().end_offset(), "Useless `private` access modifier for constant scope.");
    }
}
