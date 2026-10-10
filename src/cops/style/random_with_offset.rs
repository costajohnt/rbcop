use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/RandomWithOffset";
pub static COP: RandomWithOffset = RandomWithOffset;
pub struct RandomWithOffset;

const MSG: &str = "Prefer ranges when generating random numbers instead of integers with offsets.";

impl Cop for RandomWithOffset {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || has_block(&call) {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let name = method_name(&call);
        let args = send_args(&call);
        // (prefix source, rand bounds, corrected bounds)
        let fixed = match (name, args.as_slice()) {
            ("+" | "-", [arg]) => {
                if let (Some(offset), Some((prefix, (l, r)))) = (int(&recv), rand_call(arg)) {
                    // integer_op_rand?
                    Some((prefix, if name == "+" { (offset + l, offset + r) } else { (offset - r, offset - l) }))
                } else if let (Some((prefix, (l, r))), Some(offset)) = (rand_call(&recv), int(arg)) {
                    // rand_op_integer?
                    Some((prefix, if name == "+" { (l + offset, r + offset) } else { (l - offset, r - offset) }))
                } else {
                    return;
                }
            }
            ("succ" | "next" | "pred", []) => {
                let Some((prefix, (l, r))) = rand_call(&recv) else { return };
                Some((prefix, if name == "pred" { (l - 1, r - 1) } else { (l + 1, r + 1) }))
            }
            _ => return,
        };
        let (s, e) = send_range(&call);
        let fix = fixed.map(|(prefix, (l, r))| format!("{prefix}({l}..{r})"));
        cx.add_fix(s, e, MSG, |c| {
            if let Some(fix) = &fix {
                c.replace(s, e, fix);
            }
        });
    }
}

fn has_block(call: &ruby_prism::CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_node().is_some())
}

/// `(int $_)`
fn int(node: &Node) -> Option<i128> {
    let v = node.as_integer_node()?.value();
    let (neg, digits) = v.to_u32_digits();
    let mut n: i128 = 0;
    for d in digits.iter().rev() {
        n = n.checked_mul(1 << 32)?.checked_add(*d as i128)?;
    }
    Some(if neg { -n } else { n })
}

/// `(send {nil? (const {nil? cbase} :Random) (const {nil? cbase} :Kernel)} :rand {int (range int int)})`:
/// the `rand` prefix and the inclusive bounds of the random value.
fn rand_call(node: &Node) -> Option<(String, (i128, i128))> {
    let call = node.as_call_node()?;
    if is_csend(&call) || has_block(&call) || method_name(&call) != "rand" {
        return None;
    }
    let prefix = match call.receiver() {
        None => "rand".to_string(),
        Some(r) if is_top_const(&r, "Random") || is_top_const(&r, "Kernel") => format!("{}.rand", node_text(&r)),
        Some(_) => return None,
    };
    let args = send_args(&call);
    let [arg] = args.as_slice() else { return None };
    let bounds = if let Some(n) = int(arg) {
        (0, n - 1)
    } else {
        let range = arg.as_range_node()?;
        let (l, r) = (int(&range.left()?)?, int(&range.right()?)?);
        (l, if range.is_exclude_end() { r - 1 } else { r })
    };
    Some((prefix, bounds))
}
