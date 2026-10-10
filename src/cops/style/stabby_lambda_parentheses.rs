use crate::prelude::*;

pub const NAME: &str = "Style/StabbyLambdaParentheses";
pub static COP: StabbyLambdaParentheses = StabbyLambdaParentheses;
pub struct StabbyLambdaParentheses;

impl Cop for StabbyLambdaParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let lambda = node.as_lambda_node().unwrap();
        let Some(params) = lambda.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        if params.parameters().is_none() && params.locals().iter().next().is_none() {
            return;
        }
        let require = cx.cfg.str("EnforcedStyle") != Some("require_no_parentheses");
        let parens = params.opening_loc().zip(params.closing_loc());
        if require == parens.is_some() {
            return;
        }
        let loc = params.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let msg = if require {
            "Wrap stabby lambda arguments with parentheses."
        } else {
            "Do not wrap stabby lambda arguments with parentheses."
        };
        cx.add_fix(s, e, msg, |c| match parens {
            None => c.wrap(s, e, "(", ")"),
            Some((open, close)) => {
                c.replace(open.start_offset(), open.end_offset(), "");
                c.remove(close.start_offset(), close.end_offset());
            }
        });
    }
}
