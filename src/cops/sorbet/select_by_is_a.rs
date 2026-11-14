use crate::prelude::*;

pub const NAME: &str = "Sorbet/SelectByIsA";
pub static COP: SelectByIsA = SelectByIsA;
pub struct SelectByIsA;

impl Cop for SelectByIsA {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if !matches!(method_name(&call), "select" | "filter") || call.arguments().is_some() {
            return;
        }
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        // A `block` takes exactly `(args (arg _))`; a numblock / itblock anything.
        let params_ok = match block.parameters() {
            None => false,
            Some(Node::BlockParametersNode { .. }) => {
                let bp = block.parameters().unwrap().as_block_parameters_node().unwrap();
                bp.locals().iter().next().is_none()
                    && bp.parameters().is_some_and(|p| {
                        let all = parameter_nodes(&p);
                        all.len() == 1 && all[0].as_required_parameter_node().is_some() && p.rest().is_none()
                    })
            }
            Some(_) => true,
        };
        if !params_ok {
            return;
        }
        // (send (lvar _) {:is_a? :kind_of?} (const nil? _))
        let Some(body) = block.body().and_then(|b| b.as_statements_node()) else { return };
        let stmts: Vec<Node> = body.body().iter().collect();
        let [stmt] = stmts.as_slice() else { return };
        let Some(test) = stmt.as_call_node() else { return };
        if !matches!(method_name(&test), "is_a?" | "kind_of?")
            || is_csend(&test)
            || test.block().is_some()
            || !test.receiver().is_some_and(|r| helpers::node_types::lvar_name(&r).is_some())
        {
            return;
        }
        let args = call_args(&test);
        let [class] = args.as_slice() else { return };
        if class.as_constant_read_node().is_none() {
            return;
        }
        // The corrector calls `receiver.source`: without a receiver RuboCop crashes and loses the offense.
        let Some(receiver) = call.receiver() else { return };
        let nav = if is_csend(&call) { "&." } else { "." };
        let replacement = format!("{}{nav}grep({})", node_text(&receiver), node_text(class));
        let (s, e) = helpers::node_types::range(node);
        cx.add_fix(s, e, "Use `grep` instead of `select` when using it only for type narrowing.", |c| {
            c.replace(s, e, &replacement)
        });
    }
}
