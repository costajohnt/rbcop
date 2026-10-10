use crate::helpers::macro_scope::class_constructor;
use crate::prelude::*;

pub const NAME: &str = "Style/ConstantVisibility";
pub static COP: ConstantVisibility = ConstantVisibility;
pub struct ConstantVisibility;

/// `argument.value.to_sym if argument.type?(:sym, :str)`
fn sym_or_str_value(n: &Node) -> Option<Vec<u8>> {
    if let Some(s) = n.as_symbol_node() {
        return Some(s.unescaped().to_vec());
    }
    if crate::helpers::literal_node::parser_type(n) == "str" {
        return n.as_string_node().map(|s| s.unescaped().to_vec());
    }
    None
}

/// visibility_declaration_for: `(send nil? {:public_constant :private_constant} $...)` naming `name`.
fn declares(n: &Node, name: &[u8]) -> bool {
    let Some(c) = n.as_call_node() else { return false };
    if c.receiver().is_some() || c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    if !matches!(method_name(&c), "public_constant" | "private_constant") {
        return false;
    }
    let mut args = call_args(&c);
    if let Some(splat) = args.first().and_then(|a| a.as_splat_node()) {
        match splat.expression().and_then(|e| e.as_array_node()) {
            Some(arr) => args = arr.elements().iter().collect(),
            None => return true,
        }
    }
    args.iter().any(|a| sym_or_str_value(a).is_some_and(|v| v == name))
}

impl Cop for ConstantVisibility {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_WRITE_NODE, kind::CONSTANT_PATH_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (name, value) = match node {
            Node::ConstantWriteNode { .. } => {
                let w = node.as_constant_write_node().unwrap();
                (w.name().as_slice().to_vec(), w.value())
            }
            _ => {
                let w = node.as_constant_path_write_node().unwrap();
                let Some(name) = w.target().name() else { return };
                (name.as_slice().to_vec(), w.value())
            }
        };
        // class_or_module_scope?: through `begin` parents up to a class or module
        let scope = cx.ancestors.iter().rev().find(|a| {
            !matches!(kind::of(a), kind::STATEMENTS_NODE | kind::PARENTHESES_NODE)
        });
        if !scope.is_some_and(|s| matches!(kind::of(s), kind::CLASS_NODE | kind::MODULE_NODE)) {
            return;
        }
        // the parent's child sends: the statements around the assignment
        let siblings = cx.ancestors.last().and_then(|p| p.as_statements_node());
        if siblings.is_some_and(|s| s.body().iter().any(|n| declares(&n, &name))) {
            return;
        }
        if cx.cfg.bool("IgnoreModules").unwrap_or(false)
            && value.as_call_node().is_some_and(|c| !is_csend(&c) && class_constructor(&c))
        {
            return;
        }
        let loc = node.location();
        let name = String::from_utf8_lossy(&name);
        cx.add(
            loc.start_offset(),
            loc.end_offset(),
            format!("Explicitly make `{name}` public or private using either `#public_constant` or `#private_constant`."),
        );
    }
}
