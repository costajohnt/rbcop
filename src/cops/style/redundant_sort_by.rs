use crate::prelude::*;

pub const NAME: &str = "Style/RedundantSortBy";
pub static COP: RedundantSortBy = RedundantSortBy;
pub struct RedundantSortBy;

impl Cop for RedundantSortBy {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "sort_by" || call.arguments().is_some() {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let Some(sel) = call.message_loc() else { return };
        // The body is a single expression.
        let Some(body) = block.body() else { return };
        let body = match body.as_statements_node() {
            Some(s) if s.body().iter().count() == 1 => s.body().iter().next().unwrap(),
            Some(_) => return,
            None => return,
        };
        let msg = match block.parameters() {
            // (block $(call _ :sort_by) (args (arg $_x)) (lvar _x))
            Some(p) if p.as_block_parameters_node().is_some() => {
                let bp = p.as_block_parameters_node().unwrap();
                if bp.locals().iter().count() != 0 {
                    return;
                }
                let Some(params) = bp.parameters() else { return };
                let reqs: Vec<_> = params.requireds().iter().collect();
                if reqs.len() != 1
                    || params.optionals().iter().count() != 0
                    || crate::ast::parser_rest(params.rest()).is_some()
                    || params.posts().iter().count() != 0
                    || params.keywords().iter().count() != 0
                    || params.keyword_rest().is_some()
                    || params.block().is_some()
                {
                    return;
                }
                let Some(arg) = reqs[0].as_required_parameter_node() else { return };
                let name = std::str::from_utf8(arg.name().as_slice()).unwrap_or("");
                let Some(lvar) = body.as_local_variable_read_node() else { return };
                if lvar.name().as_slice() != name.as_bytes() {
                    return;
                }
                format!("Use `sort` instead of `sort_by {{ |{name}| {name} }}`.")
            }
            // (numblock $(call _ :sort_by) 1 (lvar :_1))
            Some(p) if p.as_numbered_parameters_node().is_some() => {
                if p.as_numbered_parameters_node().unwrap().maximum() != 1
                    || body.as_local_variable_read_node().is_none_or(|l| l.name().as_slice() != b"_1")
                {
                    return;
                }
                "Use `sort` instead of `sort_by { _1 }`.".to_string()
            }
            // (itblock $(call _ :sort_by) _ (lvar :it))
            Some(p) if p.as_it_parameters_node().is_some() => {
                if body.as_it_local_variable_read_node().is_none() {
                    return;
                }
                "Use `sort` instead of `sort_by { it }`.".to_string()
            }
            _ => return,
        };
        let (start, end) = (sel.start_offset(), block.closing_loc().end_offset());
        cx.add_fix(start, end, msg, |c| c.replace(start, end, "sort"));
    }
}
