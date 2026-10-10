use crate::helpers::add_parentheses;
use crate::prelude::*;

pub const NAME: &str = "Style/MethodDefParentheses";
pub static COP: MethodDefParentheses = MethodDefParentheses;
pub struct MethodDefParentheses;

impl Cop for MethodDefParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let params = def.parameters();
        // Parser `args` range: the parentheses when present, else the parameters (none when empty).
        let parens = def.lparen_loc().zip(def.rparen_loc()).map(|(l, r)| (l.start_offset(), r.end_offset()));
        let args = parens.or(params.as_ref().map(|p| (p.location().start_offset(), p.location().end_offset())));
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("require_parentheses");
        let multiline = args.is_some_and(|(s, e)| cx.src.line_of(s) != cx.src.line_of(e));
        if style == "require_parentheses" || (style == "require_no_parentheses_except_multiline" && multiline) {
            if parens.is_none()
                && let Some(p) = &params
            {
                // Util#add_parentheses on an `args` node.
                let (s, e) = (p.location().start_offset(), p.location().end_offset());
                let fix = add_parentheses::plan(&p.as_node(), cx.src.text).unwrap();
                cx.add_fix(s, e, "Use def with parentheses when there are parameters.", |c| add_parentheses::apply(c, &fix));
            }
            return;
        }
        if def.equal_loc().is_some() || params.as_ref().is_some_and(anonymous_arguments) {
            return;
        }
        if let Some((s, e)) = parens {
            cx.add_fix(s, e, "Use def without parentheses.", |c| {
                c.replace(s, s + 1, " ");
                c.remove(e - 1, e);
            });
        }
    }
}

/// `...`, bare `*`, `**` or `&`.
fn anonymous_arguments(p: &ruby_prism::ParametersNode) -> bool {
    p.keyword_rest().is_some_and(|k| {
        k.as_forwarding_parameter_node().is_some() || k.as_keyword_rest_parameter_node().is_some_and(|k| k.name().is_none())
    }) || crate::ast::parser_rest(p.rest()).and_then(|r| r.as_rest_parameter_node()).is_some_and(|r| r.name().is_none())
        || p.block().is_some_and(|b| b.name().is_none())
}
