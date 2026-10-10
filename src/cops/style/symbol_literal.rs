use crate::prelude::*;

pub const NAME: &str = "Style/SymbolLiteral";
pub static COP: SymbolLiteral = SymbolLiteral;
pub struct SymbolLiteral;

static WORD_LIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\A:["'][A-Za-z_][A-Za-z0-9_]*["']\z"#).unwrap());

impl Cop for SymbolLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::SYMBOL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = node_text(node);
        if !WORD_LIKE.is_match(src) {
            return;
        }
        let loc = node.location();
        let fixed: String = src.chars().filter(|&c| c != '\'' && c != '"').collect();
        cx.add_fix(loc.start_offset(), loc.end_offset(), "Do not use strings for word-like symbol literals.", |c| {
            c.replace(loc.start_offset(), loc.end_offset(), &fixed)
        });
    }
}
