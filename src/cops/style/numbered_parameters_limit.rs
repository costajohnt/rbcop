use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/NumberedParametersLimit";
pub static COP: NumberedParametersLimit = NumberedParametersLimit;
pub struct NumberedParametersLimit;

/// Distinct `_1`..`_9` local variable reads (`each_descendant(:lvar).uniq`).
#[derive(Default)]
struct Reads(Vec<Vec<u8>>);

impl<'pr> Visit<'pr> for Reads {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        let name = node.location().as_slice();
        if name.len() == 2 && name[0] == b'_' && (b'1'..=b'9').contains(&name[1]) && !self.0.iter().any(|n| n == name)
        {
            self.0.push(name.to_vec());
        }
    }
}

impl Cop for NumberedParametersLimit {
    fn ruby_range(&self) -> (f64, f64) {
        (2.7, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // parser `numblock`: the block (with its call) whose params are `_1`..`_9`.
        let params = if let Some(l) = node.as_lambda_node() {
            l.parameters()
        } else {
            let block = match node {
                Node::CallNode { .. } => node.as_call_node().unwrap().block(),
                Node::SuperNode { .. } => node.as_super_node().unwrap().block(),
                _ => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
            };
            let Some(b) = block.and_then(|b| b.as_block_node()) else { return };
            b.parameters()
        };
        if !params.is_some_and(|p| p.as_numbered_parameters_node().is_some()) {
            return;
        }
        // The whole numblock: its send's receiver and arguments count too.
        let mut reads = Reads::default();
        reads.visit(node);
        let count = reads.0.len() as i64;
        // `[cop_config.fetch('Max', 1), 9].min`, compared as it is and printed with `%i`
        let max = match cx.cfg.num("Max").unwrap_or(Num::Int(1)) {
            n if n.f64() > 9.0 => Num::Int(9),
            n => n,
        };
        if count as f64 <= max.f64() {
            return;
        }
        let parameter = if max.f64() > 1.0 { "parameters" } else { "parameter" };
        let msg = format!("Avoid using more than {} numbered {parameter}; {count} detected.", max.to_i());
        let loc = node.location();
        cx.add(loc.start_offset(), loc.end_offset(), msg);
    }
}
