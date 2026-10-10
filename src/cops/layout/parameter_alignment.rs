use crate::helpers::alignment::{self, Item};
use crate::prelude::*;

pub const NAME: &str = "Layout/ParameterAlignment";
pub static COP: ParameterAlignment = ParameterAlignment;
pub struct ParameterAlignment;

impl Cop for ParameterAlignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let Some(params) = def.parameters() else { return };
        let args = parameter_nodes(&params);
        if args.len() < 2 {
            return;
        }
        let fixed = cx.cfg.str("EnforcedStyle") == Some("with_fixed_indentation");
        let base = if fixed {
            let line = cx.src.lines[cx.src.line_of(def.def_keyword_loc().start_offset()) - 1];
            alignment::first_non_space(line) as i64 + alignment::configured_indentation_width(cx)
        } else {
            alignment::display_column(cx.src, args[0].location().start_offset()) as i64
        };
        let msg = if fixed {
            "Use one level of indentation for parameters following the first line of a multi-line method definition."
        } else {
            "Align the parameters of a method definition if they span more than one line."
        };
        let items: Vec<Item> = args.into_iter().map(Item::of).collect();
        alignment::check_alignment(cx, &items, Some(base), msg);
    }
}
