use crate::prelude::*;

pub const NAME: &str = "Layout/SpaceInLambdaLiteral";
pub static COP: SpaceInLambdaLiteral = SpaceInLambdaLiteral;
pub struct SpaceInLambdaLiteral;

impl Cop for SpaceInLambdaLiteral {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let lambda = node.as_lambda_node().unwrap();
        // node.parent.arguments?: a parameter list that is not empty (numbered/it params don't count).
        let Some(params) = lambda.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        if params.parameters().is_none() && params.locals().iter().next().is_none() {
            return;
        }
        let arrow_end = lambda.operator_loc().end_offset();
        let args = params.location();
        let spaced = args.start_offset() > arrow_end;
        match cx.cfg.str("EnforcedStyle") {
            Some("require_space") if !spaced => {
                cx.add(lambda.location().start_offset(), args.end_offset(), "Use a space between `->` and `(` in lambda literals.")
            }
            Some("require_no_space") if spaced => {
                cx.add(arrow_end, args.start_offset(), "Do not use spaces between `->` and `(` in lambda literals.")
            }
            _ => {}
        }
    }
}
