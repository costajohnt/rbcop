use crate::prelude::*;

pub const NAME: &str = "Style/EachForSimpleLoop";
pub static COP: EachForSimpleLoop = EachForSimpleLoop;
pub struct EachForSimpleLoop;

fn int_value(node: &Node) -> Option<i128> {
    let int = node.as_integer_node()?;
    let value = int.value();
    let (neg, digits) = value.to_u32_digits();
    let mut v: i128 = 0;
    for d in digits.iter().rev() {
        v = v.checked_mul(1 << 32)?.checked_add(*d as i128)?;
    }
    Some(if neg { -v } else { v })
}

impl Cop for EachForSimpleLoop {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        // (args): no parameters at all (numbered/`it` blocks are not `block` nodes)
        if let Some(p) = block.parameters() {
            let Some(bp) = p.as_block_parameters_node() else { return };
            if bp.parameters().is_some() || bp.locals().iter().next().is_some() {
                return;
            }
        }
        if method_name(&call) != "each" || call.arguments().is_some() {
            return;
        }
        // (begin (range (int $_) (int $_)))
        let Some(parens) = call.receiver().and_then(|r| r.as_parentheses_node()) else { return };
        let Some(body) = parens.body() else { return };
        let stmts: Vec<Node> = match body.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![body],
        };
        let [range] = stmts.as_slice() else { return };
        let Some(range) = range.as_range_node() else { return };
        let (Some(min), Some(max)) =
            (range.left().as_ref().and_then(int_value), range.right().as_ref().and_then(int_value))
        else {
            return;
        };
        let max = if range.is_exclude_end() { max } else { max + 1 };
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `Integer#times` for a simple loop which iterates a fixed number of times.", |c| {
            c.replace(s, e, &format!("{}.times", max - min))
        });
    }
}
