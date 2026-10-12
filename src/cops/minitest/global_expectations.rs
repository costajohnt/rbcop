use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/GlobalExpectations";
pub static COP: GlobalExpectations = GlobalExpectations;
pub struct GlobalExpectations;

impl Cop for GlobalExpectations {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !is_matcher(name) || is_csend(&call) {
            return;
        }
        let Some(receiver) = call.receiver() else { return };
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("any");
        let preferred = if style == "any" { "_" } else { style };
        // block_receiver? `(block (send nil? $dsl) _ _)` / value_receiver? `(send nil? $dsl _)`
        let dsl = |c: &ruby_prism::CallNode| {
            c.receiver().is_none() && matches!(method_name(c), "_" | "expect" | "value")
        };
        let mut dsl_selector = None;
        let method = if let Some(b) = block_call(&receiver) {
            (dsl(&b) && send_args(&b).is_empty()).then(|| {
                dsl_selector = Some(b.message_loc().unwrap());
                method_name(&b)
            })
        } else if let Some(s) = as_send(&receiver) {
            (dsl(&s) && send_args(&s).len() == 1).then(|| {
                dsl_selector = Some(s.message_loc().unwrap());
                method_name(&s)
            })
        } else {
            None
        };
        if method == Some(preferred) || (method.is_some() && style == "any") {
            return;
        }
        let (rs, re) = range(&receiver);
        let block_matcher = is_block_matcher(name);
        // receiver.lambda? (`({block numblock itblock} (send nil? :lambda) ...)`) and its body.
        let lambda_body = match &receiver {
            Node::LambdaNode { .. } => Some(receiver.as_lambda_node().unwrap().body()),
            Node::CallNode { .. } => {
                let c = receiver.as_call_node().unwrap();
                match c.block().and_then(|b| b.as_block_node()) {
                    Some(b) if method_name(&c) == "lambda" && c.receiver().is_none() => Some(b.body()),
                    _ => None,
                }
            }
            _ => None,
        };
        let lambda_src = match &lambda_body {
            Some(Some(b)) => Some(body_source(b)),
            Some(None) if block_matcher && method.is_none() => return,
            _ => None,
        };
        let shown = match method {
            Some(_) => preferred.to_string(),
            None if block_matcher => {
                let body = lambda_src.unwrap_or(&cx.src.text[rs..re]);
                format!("{preferred} {{ {body} }}")
            }
            None => format!("{preferred}({})", &cx.src.text[rs..re]),
        };
        let message = format!("Use `{shown}` instead.");
        // `receiver.block_type?` excludes numblock/itblock lambdas, which get wrapped instead.
        let numbered = match &receiver {
            Node::LambdaNode { .. } => matches!(
                receiver.as_lambda_node().unwrap().parameters(),
                Some(Node::NumberedParametersNode { .. }) | Some(Node::ItParametersNode { .. })
            ),
            _ => block_call(&receiver).is_none(),
        };
        cx.add_fix(rs, re, message, |c| {
            if let Some(sel) = dsl_selector {
                c.replace(sel.start_offset(), sel.end_offset(), preferred);
            } else if block_matcher {
                match &lambda_src {
                    Some(body) if !numbered => c.replace(rs, re, &format!("{preferred} {{ {body} }}")),
                    _ => c.wrap(rs, re, &format!("{preferred} {{ "), " }"),
                }
            } else {
                c.wrap(rs, re, &format!("{preferred}("), ")");
            }
        });
    }
}

/// Parser `body.source` of a block/lambda body.
fn body_source<'pr>(body: &Node<'pr>) -> &'pr str {
    let (s, e) = body_range(body);
    let full = body.location();
    let base = full.start_offset();
    &node_text(body)[s - base..e - base]
}
