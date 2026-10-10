use crate::prelude::*;

pub const NAME: &str = "Lint/FloatComparison";
pub static COP: FloatComparison = FloatComparison;
pub struct FloatComparison;

/// First child of a parser `begin` (parenthesized expression).
fn begin_first<'pr>(n: &Node<'pr>) -> Option<Option<Node<'pr>>> {
    let p = n.as_parentheses_node()?;
    Some(p.body().and_then(|b| match b.as_statements_node() {
        Some(s) => s.body().iter().next(),
        None => Some(b),
    }))
}

fn int_sign(n: &Node) -> Option<std::cmp::Ordering> {
    let v = n.as_integer_node()?.value();
    let (neg, digits) = v.to_u32_digits();
    Some(if digits.iter().all(|&d| d == 0) {
        std::cmp::Ordering::Equal
    } else if neg {
        std::cmp::Ordering::Less
    } else {
        std::cmp::Ordering::Greater
    })
}

fn is_zero_numeric(n: &Node) -> bool {
    if let Some(o) = int_sign(n) {
        return o.is_eq();
    }
    if let Some(f) = n.as_float_node() {
        return f.value() == 0.0;
    }
    if let Some(r) = n.as_rational_node() {
        return r.numerator().to_u32_digits().1.iter().all(|&d| d == 0);
    }
    if let Some(i) = n.as_imaginary_node() {
        return is_zero_numeric(&i.numeric());
    }
    false
}

fn literal_safe(n: Option<&Node>) -> bool {
    let Some(n) = n else { return false };
    if let Some(first) = begin_first(n) {
        return literal_safe(first.as_ref());
    }
    is_zero_numeric(n) || n.as_nil_node().is_some()
}

fn is_float(n: Option<&Node>) -> bool {
    let Some(n) = n else { return false };
    if n.as_float_node().is_some() {
        return true;
    }
    if let Some(first) = begin_first(n) {
        return is_float(first.as_ref());
    }
    match n.as_call_node() {
        Some(c) if !is_csend(&c) => float_send(&c),
        _ => false,
    }
}

fn float_send(c: &ruby_prism::CallNode) -> bool {
    let name = method_name(c);
    let first = call_args(c).into_iter().next();
    if matches!(name, "+" | "-" | "*" | "/" | "%" | "**") {
        return is_float(c.receiver().as_ref()) || is_float(first.as_ref());
    }
    if matches!(name, "to_f" | "Float" | "fdiv") {
        return true;
    }
    let Some(recv) = c.receiver().filter(|r| r.as_float_node().is_some()) else { return false };
    match name {
        // `@-` in FLOAT_INSTANCE_METHODS never matches a method name.
        "abs" | "magnitude" | "modulo" | "next_float" | "prev_float" | "quo" => true,
        "angle" | "arg" | "phase" => recv.as_float_node().unwrap().value() < 0.0,
        "ceil" | "floor" | "round" | "truncate" => first.as_ref().and_then(int_sign).is_some_and(|o| o.is_gt()),
        _ => false,
    }
}

impl Cop for FloatComparison {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::CASE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(case) = node.as_case_node() {
            for when in case.conditions().iter() {
                for cond in when.as_when_node().unwrap().conditions().iter() {
                    if is_float(Some(&cond)) && !literal_safe(Some(&cond)) {
                        let l = cond.location();
                        cx.add(
                            l.start_offset(),
                            l.end_offset(),
                            "Avoid float literal comparisons in case statements as they are unreliable.",
                        );
                    }
                }
            }
            return;
        }
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "==" | "!=" | "eql?" | "equal?") {
            return;
        }
        let args = send_args(&call);
        if args.len() != 1 {
            return;
        }
        let lhs = call.receiver();
        let rhs = Some(&args[0]);
        if literal_safe(lhs.as_ref()) || literal_safe(rhs) {
            return;
        }
        if is_float(lhs.as_ref()) || is_float(rhs) {
            let msg = if name == "!=" {
                "Avoid inequality comparisons of floats as they are unreliable."
            } else {
                "Avoid equality comparisons of floats as they are unreliable."
            };
            let (s, e) = send_range(&call);
            cx.add(s, e, msg);
        }
    }
}
