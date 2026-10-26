use crate::prelude::*;

pub const NAME: &str = "Style/BitwisePredicate";
pub static COP: BitwisePredicate = BitwisePredicate;
pub struct BitwisePredicate;

impl Cop for BitwisePredicate {
    fn correctable(&self) -> bool {
        true
    }
    fn ruby_range(&self) -> (f64, f64) {
        (2.5, f64::MAX)
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "!=" | "==" | ">" | ">=" | "positive?" | "zero?") || is_csend(&call) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // `{(begin (send _ :& _)) (send _ :& _)}`
        let parenthesized = recv.as_parentheses_node().is_some();
        let bit = if let Some(paren) = recv.as_parentheses_node() {
            let Some(stmts) = paren.body().and_then(|b| b.as_statements_node()) else { return };
            let list: Vec<Node> = stmts.body().iter().collect();
            let [bit] = list.as_slice() else { return };
            let Some(bit) = bit.as_call_node() else { return };
            bit
        } else {
            let Some(bit) = recv.as_call_node() else { return };
            bit
        };
        if is_csend(&bit) || method_name(&bit) != "&" || bit.block().is_some() {
            return;
        }
        let bit_args = call_args(&bit);
        let ([rhs], Some(lhs)) = (bit_args.as_slice(), bit.receiver()) else { return };
        if call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
            return;
        }
        let args = call_args(&call);
        let int = |v: i128| matches!(args.as_slice(), [a] if int_literal_value(a) == Some(v));
        let src = cx.src;
        let range = |n: &Node| (n.location().start_offset(), n.location().end_offset());
        let preferred = if (name == "positive?" && args.is_empty())
            || (name == ">" && int(0))
            || (name == ">=" && int(1))
            || (name == "!=" && int(0))
        {
            "anybits?"
        } else if name == "==" && parenthesized && args.len() == 1 && {
            let flags = range(&args[0]);
            helpers::node_equality::same(src, range(rhs), flags)
                || helpers::node_equality::same(src, range(&lhs), flags)
        } {
            "allbits?"
        } else if (name == "zero?" && args.is_empty()) || (name == "==" && int(0)) {
            "nobits?"
        } else {
            return;
        };
        let (l, r) = (node_text(&lhs), node_text(rhs));
        let preferred = if preferred == "allbits?" && l == node_text(&args[0]) {
            format!("{r}.allbits?({l})")
        } else {
            format!("{l}.{preferred}({r})")
        };
        let (s, e) = send_range(&call);
        let msg = format!("Replace with `{preferred}` for comparison with bit flags.");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &preferred));
    }
}
