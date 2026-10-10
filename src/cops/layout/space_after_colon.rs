use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceAfterColon";
pub static COP: SpaceAfterColon = SpaceAfterColon;
pub struct SpaceAfterColon;

impl Cop for SpaceAfterColon {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ASSOC_NODE, kind::OPTIONAL_KEYWORD_PARAMETER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // The colon ends the label (`a:` / `"a":`) of a pair, or the name of a `kwoptarg`.
        let colon_end = if let Some(pair) = node.as_assoc_node() {
            if pair.operator_loc().is_some() || pair.value().as_implicit_node().is_some() {
                return;
            }
            pair.key().location().end_offset()
        } else {
            node.as_optional_keyword_parameter_node().unwrap().name_loc().end_offset()
        };
        // Ruby's /\s/ on the next character; end of file counts as missing.
        if !matches!(cx.src.text.as_bytes().get(colon_end), Some(b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')) {
            cx.add(colon_end - 1, colon_end, "Space missing after colon.");
        }
    }
}
