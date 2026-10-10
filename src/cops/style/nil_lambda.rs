use crate::helpers::range_help::{Surrounding, range_with_surrounding_space};
use crate::prelude::*;

pub const NAME: &str = "Style/NilLambda";
pub static COP: NilLambda = NilLambda;
pub struct NilLambda;

/// `{ ({return next break} nil) (nil) }`; the bool is "is a return".
fn nil_return(node: &Node) -> Option<bool> {
    if node.as_nil_node().is_some() {
        return Some(false);
    }
    let (args, is_return) = match kind::of(node) {
        kind::RETURN_NODE => (node.as_return_node().unwrap().arguments(), true),
        kind::NEXT_NODE => (node.as_next_node().unwrap().arguments(), false),
        kind::BREAK_NODE => (node.as_break_node().unwrap().arguments(), false),
        _ => return None,
    };
    let args: Vec<Node> = args?.arguments().iter().collect();
    matches!(args.as_slice(), [a] if a.as_nil_node().is_some()).then_some(is_return)
}

impl Cop for NilLambda {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (lambda, body, open) = if let Some(l) = node.as_lambda_node() {
            if l.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
                return;
            }
            (true, l.body(), l.opening_loc())
        } else {
            let call = node.as_call_node().unwrap();
            let Some(b) = call.block().and_then(|b| b.as_block_node()) else { return };
            // on_block only: numblock/itblock are not checked.
            if b.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
                return;
            }
            let name = method_name(&call);
            // BlockNode#lambda? is any call named `lambda`; proc? wants `proc` / `Proc.new` without args.
            let lambda = name == "lambda";
            let proc = call.arguments().is_none()
                && ((call.receiver().is_none() && name == "proc")
                    || (name == "new" && call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Proc"))));
            if !lambda && !proc {
                return;
            }
            (lambda, b.body(), b.opening_loc())
        };
        let Some(stmt) = body.and_then(|b| b.as_statements_node()).and_then(|s| {
            let v: Vec<Node> = s.body().iter().collect();
            (v.len() == 1).then(|| v.into_iter().next().unwrap())
        }) else {
            return;
        };
        let Some(is_return) = nil_return(&stmt) else { return };
        if is_return && !lambda {
            return;
        }
        let loc = node.location();
        let msg = format!("Use an empty {} instead of always returning nil.", if lambda { "lambda" } else { "proc" });
        let src = cx.src;
        let (bs, be) = (stmt.location().start_offset(), stmt.location().end_offset());
        let single = src.line_of(open.start_offset()) == src.line_of(loc.end_offset());
        let (rs, re) = if single {
            range_with_surrounding_space(src.text, bs, be, Surrounding::default())
        } else {
            helpers::whole_lines::range_by_whole_lines(src.text, bs, be, true)
        };
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| c.remove(rs, re));
    }
}
