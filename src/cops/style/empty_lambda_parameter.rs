use crate::helpers::empty_parameter::empty_params;
use crate::prelude::*;

pub const NAME: &str = "Style/EmptyLambdaParameter";
pub static COP: EmptyLambdaParameter = EmptyLambdaParameter;
pub struct EmptyLambdaParameter;

impl Cop for EmptyLambdaParameter {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let lambda = node.as_lambda_node().unwrap();
        let Some((start, end)) = empty_params(lambda.parameters()) else { return };
        let arrow = lambda.operator_loc().end_offset();
        cx.add_fix(start, end, "Omit parentheses for the empty lambda parameters.", |c| c.remove(arrow, end));
    }
}
