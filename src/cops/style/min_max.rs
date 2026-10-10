use crate::prelude::*;

pub const NAME: &str = "Style/MinMax";
pub static COP: MinMax = MinMax;
pub struct MinMax;

/// `(send [$_receiver !nil?] :<name>)`
fn receiver<'pr>(n: &Node<'pr>, name: &str) -> Option<Node<'pr>> {
    let c = n.as_call_node()?;
    if method_name(&c) != name || is_csend(&c) || c.arguments().is_some() || c.block().is_some() {
        return None;
    }
    c.receiver()
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

impl Cop for MinMax {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE, kind::RETURN_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (elems, whole): (Vec<Node>, bool) = if let Some(a) = node.as_array_node() {
            (a.elements().iter().collect(), true)
        } else {
            let r = node.as_return_node().unwrap();
            (r.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default(), false)
        };
        let [first, last] = elems.as_slice() else { return };
        let (Some(r1), Some(r2)) = (receiver(first, "min"), receiver(last, "max")) else { return };
        if !helpers::node_equality::same(cx.src, range(&r1), range(&r2)) {
            return;
        }
        let (s, e) = if whole { range(node) } else { (range(first).0, range(last).1) };
        let recv_src = node_text(&r1);
        let msg = format!("Use `{recv_src}.minmax` instead of `{}`.", &cx.src.text[s..e]);
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &format!("{recv_src}.minmax")));
    }
}
