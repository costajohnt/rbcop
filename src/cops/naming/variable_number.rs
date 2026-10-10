use crate::helpers::configurable_naming::{self as naming, VarKind};
use crate::prelude::*;

pub const NAME: &str = "Naming/VariableNumber";
pub static COP: VariableNumber = VariableNumber;
pub struct VariableNumber;

static KINDS: LazyLock<Vec<usize>> = LazyLock::new(|| {
    let mut k = naming::VARIABLE_KINDS.to_vec();
    k.extend([kind::DEF_NODE, kind::SYMBOL_NODE]);
    k
});

impl Cop for VariableNumber {
    fn kinds(&self) -> &'static [usize] {
        KINDS.as_slice()
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (name, start, end, what) = match node {
            Node::DefNode { .. } => {
                let def = node.as_def_node().unwrap();
                let loc = def.name_loc();
                let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
                if naming::allowed_identifier(cx.cfg, &name) || !cx.cfg.bool("CheckMethodNames").unwrap_or(false) {
                    return;
                }
                if def.receiver().is_some() && naming::class_emitter_method(cx.ancestors, &name) {
                    return;
                }
                (name, loc.start_offset(), loc.end_offset(), "method name")
            }
            Node::SymbolNode { .. } => {
                let sym = node.as_symbol_node().unwrap();
                let name = String::from_utf8_lossy(sym.unescaped()).into_owned();
                if name.is_empty() || naming::allowed_identifier(cx.cfg, &name) || !cx.cfg.bool("CheckSymbols").unwrap_or(false) {
                    return;
                }
                let loc = node.location();
                // a label's parser `sym` excludes the trailing colon
                let label = sym.closing_loc().is_some_and(|c| c.as_slice().ends_with(b":"));
                (name, loc.start_offset(), loc.end_offset() - label as usize, "symbol")
            }
            _ => {
                let Some((k, name, start, end)) = naming::variable(node, cx.ancestors) else { return };
                // on_arg is aliased for arg/lvasgn/ivasgn/cvasgn/gvasgn only
                if matches!(k, VarKind::OtherArg | VarKind::Lvar) || naming::allowed_identifier(cx.cfg, &name) {
                    return;
                }
                (name, start, end, "variable")
            }
        };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("normalcase");
        if naming::numbering_valid(style, &name) || helpers::allowed::matches_allowed_pattern(cx.cfg, &name) {
            return;
        }
        cx.add(start, end, format!("Use {style} for {what} numbers."));
    }
}
