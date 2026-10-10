use crate::helpers::alignment;
use crate::prelude::*;

pub const NAME: &str = "Layout/FirstParameterIndentation";
pub static COP: FirstParameterIndentation = FirstParameterIndentation;
pub struct FirstParameterIndentation;

impl Cop for FirstParameterIndentation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let Some(params) = def.parameters() else { return };
        let Some(lparen) = def.lparen_loc() else { return };
        let args = parameter_nodes(&params);
        let align_parens = cx.cfg.str("EnforcedStyle") == Some("align_parentheses");
        if args.len() >= 2 && align_parens {
            let pa = cx.config.cop("Layout/ParameterAlignment");
            if pa.enabled && pa.str("EnforcedStyle") == Some("with_fixed_indentation") {
                return;
            }
        }
        let Some(first) = args.first() else { return };
        let src = cx.src;
        let (paren, start) = (lparen.start_offset(), first.location().start_offset());
        if src.line_of(paren) == src.line_of(start) {
            return;
        }
        let base = if align_parens {
            src.column(paren)
        } else {
            alignment::first_non_space(src.lines[src.line_of(paren) - 1])
        };
        let width = alignment::configured_indentation_width(cx);
        let delta = base as i64 + width - src.column(start) as i64;
        if delta == 0 {
            return;
        }
        let desc = if align_parens {
            "the position of the opening parenthesis"
        } else {
            "the start of the line where the left parenthesis is"
        };
        let msg = format!("Use {width} spaces for indentation in method args, relative to {desc}.");
        let end = first.location().end_offset();
        let correctable = alignment::corrects(cx, start, end, Some(first), delta);
        cx.add_with(start, end, msg, correctable);
    }
}
