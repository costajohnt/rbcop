use crate::prelude::*;

pub const NAME: &str = "Performance/ZipWithoutBlock";
pub static COP: ZipWithoutBlock = ZipWithoutBlock;
pub struct ZipWithoutBlock;

impl Cop for ZipWithoutBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if (name != "map" && name != "collect") || call.receiver().is_none() || !send_args(&call).is_empty() {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        // The body: `[x]`, one array of one local variable.
        let Some(body) = block.body().and_then(|b| b.as_statements_node()) else { return };
        let stmts: Vec<Node> = body.body().iter().collect();
        let [array] = stmts.as_slice() else { return };
        let Some(array) = array.as_array_node() else { return };
        let elements: Vec<Node> = array.elements().iter().collect();
        let [element] = elements.as_slice() else { return };
        let params = block.parameters();
        let ok = match &params {
            // (block (call ...) (args (arg _)) (array (lvar _)))
            Some(p) if p.as_block_parameters_node().is_some() => {
                let bp = p.as_block_parameters_node().unwrap();
                let single = bp.parameters().is_some_and(|ps| {
                    ps.requireds().iter().count() == 1
                        && ps.requireds().iter().all(|r| r.as_required_parameter_node().is_some())
                        && ps.optionals().iter().count() == 0
                        // `|e,|` is `(args (arg :e))` too.
                        && ps.rest().is_none_or(|r| r.as_implicit_rest_node().is_some())
                        && ps.posts().iter().count() == 0
                        && ps.keywords().iter().count() == 0
                        && ps.keyword_rest().is_none()
                        && ps.block().is_none()
                });
                single && bp.locals().iter().count() == 0 && element.as_local_variable_read_node().is_some()
            }
            // (numblock (call ...) 1 (array (lvar _)))
            Some(p) if p.as_numbered_parameters_node().is_some() => {
                p.as_numbered_parameters_node().unwrap().maximum() == 1 && element.as_local_variable_read_node().is_some()
            }
            // (itblock (call ...) :it (array (lvar _)))
            Some(p) if p.as_it_parameters_node().is_some() => {
                element.as_it_local_variable_read_node().is_some() || element.as_local_variable_read_node().is_some()
            }
            _ => false,
        };
        if !ok {
            return;
        }
        let start = call.message_loc().unwrap().start_offset();
        let end = block.closing_loc().end_offset();
        cx.add_fix(start, end, "Use `zip` without a block argument instead.", |c| c.replace(start, end, "zip"));
    }
}
