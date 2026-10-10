use crate::helpers::string_help;
use crate::prelude::*;

pub const NAME: &str = "Style/CharacterLiteral";
pub static COP: CharacterLiteral = CharacterLiteral;
pub struct CharacterLiteral;

impl Cop for CharacterLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(s) = string_help::delimited_str(node) else { return };
        if s.opening_loc().is_none_or(|o| o.as_slice() != b"?") || string_help::in_regexp(cx.ancestors) {
            return;
        }
        let src = node_text(node);
        if !(2..=3).contains(&src.chars().count()) {
            return;
        }
        let string = &src[1..];
        let len = string.chars().count();
        let replacement = if len == 2 || string == "'" {
            Some(format!("\"{string}\""))
        } else if len == 1 {
            Some(format!("'{string}'"))
        } else {
            None
        };
        let loc = node.location();
        let (start, end) = (loc.start_offset(), loc.end_offset());
        cx.add_fix(start, end, "Do not use the character literal - use string literal instead.", |c| {
            if let Some(r) = replacement {
                c.replace(start, end, &r);
            }
        });
    }
}
