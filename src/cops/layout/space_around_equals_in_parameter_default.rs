use crate::helpers::parser_tokens::is_ruby_space;
use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceAroundEqualsInParameterDefault";
pub static COP: SpaceAroundEqualsInParameterDefault = SpaceAroundEqualsInParameterDefault;
pub struct SpaceAroundEqualsInParameterDefault;

impl Cop for SpaceAroundEqualsInParameterDefault {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::OPTIONAL_PARAMETER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let opt = node.as_optional_parameter_node().unwrap();
        let b = cx.src.text.as_bytes();
        let space_after = |off: usize| b.get(off).is_some_and(|&c| is_ruby_space(c));
        // Tokens: arg, `=`, first token of the value.
        let arg_end = opt.name_loc().end_offset();
        let eq_end = opt.operator_loc().end_offset();
        let value_start = opt.value().location().start_offset();
        let (arg_sp, eq_sp) = (space_after(arg_end), space_after(eq_end));
        let space = cx.cfg.str("EnforcedStyle").unwrap_or("space") == "space";
        if (space && arg_sp && eq_sp) || (!space && !arg_sp && !eq_sp) {
            return cx.correct_style_detected();
        }
        let msg = format!("Surrounding space {} in default value assignment.", if space { "missing" } else { "detected" });
        // `range.source.match(/=\s*(\S+)/)`: whatever non-space text follows the `=` is kept.
        let range_src = cx.src.slice(arg_end, value_start);
        let after_eq = range_src.split_once('=').map_or("", |(_, r)| r.trim_start_matches(|c: char| is_ruby_space(c as u8)));
        let rest: String = after_eq.chars().take_while(|&c| !(c.is_ascii() && is_ruby_space(c as u8))).collect();
        let replacement = format!("{}{rest}", if space { " = " } else { "=" });
        if cx.block_runs(arg_end, value_start) {
            cx.opposite_style_detected();
        }
        cx.add_fix(arg_end, value_start, msg, |c| c.replace(arg_end, value_start, &replacement));
    }
}
