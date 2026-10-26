use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/StabbyLambdaParentheses";
pub static COP: StabbyLambdaParentheses = StabbyLambdaParentheses;
pub struct StabbyLambdaParentheses;

/// `arguments_require_parentheses?`: a block, lambda or braced hash among the descendants.
#[derive(Default)]
struct NeedsParens(bool);

impl<'pr> Visit<'pr> for NeedsParens {
    fn visit_block_node(&mut self, _: &ruby_prism::BlockNode<'pr>) {
        self.0 = true;
    }
    fn visit_lambda_node(&mut self, _: &ruby_prism::LambdaNode<'pr>) {
        self.0 = true;
    }
    fn visit_hash_node(&mut self, _: &ruby_prism::HashNode<'pr>) {
        self.0 = true;
    }
}

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
        // The parser `args` children: the parameters, then the shadow arguments.
        let first = params.parameters().map(|p| p.location().start_offset());
        let first = first.or_else(|| params.locals().iter().next().map(|l| l.location().start_offset()));
        let last = params.locals().iter().last().map(|l| l.location().end_offset());
        let last = last.or_else(|| params.parameters().map(|p| p.location().end_offset()));
        let (Some(first), Some(last)) = (first, last) else { return };
        if let Some((open, close)) = &parens {
            // redundant_parentheses?: not when the arguments need them or a comment sits inside.
            let mut v = NeedsParens::default();
            v.visit_block_parameters_node(&params);
            let src = &cx.src.text;
            if v.0
                || !src[open.end_offset()..first].trim().is_empty()
                || !src[last..close.start_offset()].trim().is_empty()
            {
                return;
            }
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
                c.remove(open.start_offset(), first);
                c.remove(last, close.end_offset());
            }
        });
    }
}
