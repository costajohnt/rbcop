use crate::prelude::*;

pub const NAME: &str = "Style/ComparableBetween";
pub static COP: ComparableBetween = ComparableBetween;
pub struct ComparableBetween;

type R = (usize, usize);

fn range(n: &Node) -> R {
    (n.location().start_offset(), n.location().end_offset())
}

/// A plain binary send `recv op arg`: (op, recv, arg).
fn comparison(node: &Node) -> Option<(&'static str, R, R)> {
    let call = node.as_call_node()?;
    if is_csend(&call) || call.block().is_some() {
        return None;
    }
    let op = match method_name(&call) {
        ">=" => ">=",
        "<=" => "<=",
        _ => return None,
    };
    let args = call_args(&call);
    let [arg] = args.as_slice() else { return None };
    Some((op, range(&call.receiver()?), range(arg)))
}

impl Cop for ComparableBetween {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::AND_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let and = node.as_and_node().unwrap();
        let (Some(a), Some(b)) = (comparison(&and.left()), comparison(&and.right())) else { return };
        let src = cx.src;
        let same = |x: R, y: R| helpers::node_equality::same(src, x, y);
        // `{$_value :>= $_min | $_min :<= $_value}` and its mirror: the value side of each send.
        let min_value = |(op, recv, arg): (&str, R, R)| if op == ">=" { recv } else { arg };
        let max_value = |(op, recv, arg): (&str, R, R)| if op == "<=" { recv } else { arg };
        let mut found = Vec::new();
        if same(min_value(a), max_value(b)) {
            found.push(((a.1, a.2), (b.1, b.2)));
        }
        if same(max_value(a), min_value(b)) {
            found.push(((b.1, b.2), (a.1, a.2)));
        }
        let (s, e) = range(node);
        for (min_and_value, max_and_value) in found {
            let mv = [min_and_value.0, min_and_value.1];
            let xv = [max_and_value.0, max_and_value.1];
            let Some(&value) = mv.iter().find(|m| xv.iter().any(|x| helpers::node_equality::same_hash(src, **m, *x))) else { continue };
            let min = mv.iter().copied().find(|m| !same(*m, value)).unwrap_or(value);
            let max = xv.iter().copied().find(|m| !same(*m, value)).unwrap_or(value);
            let t = |r: R| src.slice(r.0, r.1);
            let prefer = format!("{}.between?({}, {})", t(value), t(min), t(max));
            cx.add_fix(s, e, format!("Prefer `{prefer}` over logical comparison."), |c| c.replace(s, e, &prefer));
        }
    }
}
