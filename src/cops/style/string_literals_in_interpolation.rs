use crate::helpers::string_help;
use crate::prelude::*;

pub const NAME: &str = "Style/StringLiteralsInInterpolation";
pub static COP: StringLiteralsInInterpolation = StringLiteralsInInterpolation;
pub struct StringLiteralsInInterpolation;

impl Cop for StringLiteralsInInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::STRING_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(s) = string_help::delimited_str(node) else { return };
        if !string_help::inside_interpolation(cx.ancestors) {
            return;
        }
        let single = cx.cfg.str("EnforcedStyle") != Some("double_quotes");
        let binary = cx.src.binary;
        if !string_help::wrong_quotes(node_text(node), single) {
            return;
        }
        let msg = if single {
            "Prefer single-quoted strings inside interpolations."
        } else {
            "Prefer double-quoted strings inside interpolations."
        };
        let loc = node.location();
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| string_help::correct(c, &s, single, binary));
    }
}
