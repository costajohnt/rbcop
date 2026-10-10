use crate::prelude::*;

pub const NAME: &str = "Style/SlicingWithRange";
pub static COP: SlicingWithRange = SlicingWithRange;
pub struct SlicingWithRange;

fn int_is(node: &Node, want: i32) -> bool {
    node.as_integer_node().is_some_and(|i| {
        let v: Result<i32, _> = i.value().try_into();
        v.is_ok_and(|v| v == want)
    })
}

/// NodePattern `nil` is a `(nil)` literal node, not a missing child.
fn nil_lit(node: &Option<Node>) -> bool {
    node.as_ref().is_some_and(|n| n.as_nil_node().is_some())
}

fn range(node: &Option<Node>) -> (usize, usize) {
    let l = node.as_ref().unwrap().location();
    (l.start_offset(), l.end_offset())
}

impl Cop for SlicingWithRange {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.6, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let args = send_args(&call);
        if method_name(&call) != "[]" || args.len() != 1 {
            return;
        }
        let Some(rng) = args[0].as_range_node() else { return };
        let (lo, hi, op) = (rng.left(), rng.right(), text(&rng.operator_loc()));
        let excl = rng.is_exclude_end();
        let dot = call.call_operator_loc();
        let (off_s, off_e) = match (&dot, call.message_loc()) {
            (Some(d), _) => (d.start_offset(), send_range(&call).1),
            (None, Some(sel)) => (sel.start_offset(), sel.end_offset()),
            (None, None) => return,
        };
        let offense_src = &cx.src.text[off_s..off_e];
        let current = if dot.is_some() { node_text(&args[0]) } else { offense_src };
        let partial = |prefer: String| {
            let prefer = if dot.is_some() { prefer } else { format!("[{prefer}]") };
            format!("Prefer `{prefer}` over `{current}`.")
        };
        let till_end = if excl { nil_lit(&hi) } else { nil_lit(&hi) || hi.as_ref().is_some_and(|h| int_is(h, -1)) };
        let (msg, removal) = if lo.as_ref().is_some_and(|l| int_is(l, 0)) && till_end {
            (format!("Remove the useless `{offense_src}`."), (off_s, off_e))
        } else if lo.is_some() && till_end {
            (partial(format!("{}{op}", node_text(lo.as_ref().unwrap()))), range(&hi))
        } else if !excl && nil_lit(&lo) && hi.is_some() && cx.config.target_ruby >= 2.7 {
            (partial(format!("{op}{}", node_text(hi.as_ref().unwrap()))), range(&lo))
        } else {
            return;
        };
        if removal != (off_s, off_e) && dot.is_some() && call.opening_loc().is_none() {
            return;
        }
        cx.add_fix(off_s, off_e, msg, |c| c.remove(removal.0, removal.1));
    }
}
