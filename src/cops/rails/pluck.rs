use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/Pluck";
pub static COP: Pluck = Pluck;
pub struct Pluck;

impl Cop for Pluck {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        // node.each_ancestor(:any_block).first&.receiver
        if helpers::rails::nearest_block_owner(cx.ancestors).is_some_and(|o| o.is_some_and(|c| c.receiver().is_some())) {
            return;
        }
        // (any_block (call _ {:map :collect}) $_argument (send lvar :[] $_key))
        if !matches!(method_name(&call), "map" | "collect") || !send_args(&call).is_empty() {
            return;
        }
        let stmts = helpers::rails::body_statements(block.body());
        let [body] = stmts.as_slice() else { return };
        let Some(idx) = helpers::rails::send_node(body) else { return };
        if method_name(&idx) != "[]" || !idx.receiver().is_some_and(|r| r.as_local_variable_read_node().is_some() || r.as_it_local_variable_read_node().is_some()) {
            return;
        }
        let iargs = send_args(&idx);
        let [key] = iargs.as_slice() else { return };
        if key.as_regular_expression_node().is_some() || key.as_interpolated_regular_expression_node().is_some() {
            return;
        }
        // use_one_block_argument? and the block argument's name.
        let arg = match block.parameters() {
            Some(p) if p.as_numbered_parameters_node().is_some() => {
                if p.as_numbered_parameters_node().unwrap().maximum() != 1 {
                    return;
                }
                "_1".to_string()
            }
            Some(p) if p.as_it_parameters_node().is_some() => "it".to_string(),
            Some(p) => {
                let Some(bp) = p.as_block_parameters_node() else { return };
                let Some(ps) = bp.parameters() else { return };
                let nodes = parameter_nodes(&ps);
                let [one] = nodes.as_slice() else { return };
                node_text(one).to_string()
            }
            None => return,
        };
        // use_block_argument_in_key?
        if node_text(key) == arg {
            return;
        }
        struct Lvars<'a>(&'a str, bool);
        impl<'pr> Visit<'pr> for Lvars<'_> {
            fn visit_local_variable_read_node(&mut self, l: &ruby_prism::LocalVariableReadNode<'pr>) {
                self.1 |= l.name().as_slice() == self.0.as_bytes();
            }
            fn visit_it_local_variable_read_node(&mut self, _: &ruby_prism::ItLocalVariableReadNode<'pr>) {
                self.1 |= self.0 == "it";
            }
        }
        let mut lv = Lvars(&arg, false);
        // each_descendant excludes the key itself.
        if key.as_local_variable_read_node().is_none() {
            lv.visit(key);
        }
        if lv.1 {
            return;
        }
        let s = call.message_loc().unwrap().start_offset();
        let e = node.location().end_offset();
        let replacement = format!("pluck({})", node_text(key));
        let msg = format!("Prefer `{replacement}` over `{}`.", cx.src.slice(s, e));
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
