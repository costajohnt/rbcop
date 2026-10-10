use crate::helpers::allowed::{allowed_method, matches_allowed_pattern};
use crate::helpers::code_length::{Measured, check};
use crate::prelude::*;

pub const NAME: &str = "Metrics/MethodLength";
pub static COP: MethodLength = MethodLength;
pub struct MethodLength;

fn allowed(cx: &Cx, name: &str) -> bool {
    allowed_method(cx.cfg, name) || matches_allowed_pattern(cx.cfg, name)
}

/// `value` of a rubocop-ast `basic_literal?` as `to_s`.
fn basic_literal_value(node: &Node) -> Option<String> {
    Some(match kind::of(node) {
        kind::STRING_NODE if !is_parser_dstr(node) => {
            String::from_utf8_lossy(node.as_string_node()?.unescaped()).into_owned()
        }
        kind::SYMBOL_NODE => String::from_utf8_lossy(node.as_symbol_node()?.unescaped()).into_owned(),
        kind::INTEGER_NODE | kind::FLOAT_NODE => node_text(node).replace('_', ""),
        kind::TRUE_NODE => "true".into(),
        kind::FALSE_NODE => "false".into(),
        kind::NIL_NODE => String::new(),
        _ => return None,
    })
}

impl Cop for MethodLength {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let body = if let Some(def) = node.as_def_node() {
            if allowed(cx, &String::from_utf8_lossy(def.name().as_slice())) {
                return;
            }
            def.body()
        } else {
            let call = node.as_call_node().unwrap();
            let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
            if call.name().as_slice() != b"define_method" {
                return;
            }
            if let Some(first) = call_args(&call).first()
                && let Some(v) = basic_literal_value(first)
                && allowed(cx, &v)
            {
                return;
            }
            block.body()
        };
        let loc = node.location();
        let range = (loc.start_offset(), loc.end_offset());
        check(cx, Measured::Body { root: node, body }, range, range, |len, max| {
            format!("Method has too many lines. [{len}/{max}]")
        });
    }
}
