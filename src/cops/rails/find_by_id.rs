use crate::prelude::*;

pub const NAME: &str = "Rails/FindById";
pub static COP: FindById = FindById;
pub struct FindById;

/// (hash (pair (sym :id) $_))
fn id_value<'pr>(hash: &Node<'pr>) -> Option<Node<'pr>> {
    let elems = helpers::rails::hash_elements(hash)?;
    let [only] = elems.as_slice() else { return None };
    let p = only.as_assoc_node()?;
    (helpers::rails::sym_value(&p.key()).as_deref() == Some("id")).then(|| p.value())
}

impl Cop for FindById {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let args = send_args(&call);
        let (_, end) = send_range(&call);
        let found: Option<(usize, Node)> = match name {
            "take!" if args.is_empty() => call.receiver().and_then(|r| {
                let w = r.as_call_node()?;
                if w.block().is_some_and(|b| b.as_block_node().is_some()) || method_name(&w) != "where" {
                    return None;
                }
                let wa = send_args(&w);
                let [h] = wa.as_slice() else { return None };
                Some((w.message_loc()?.start_offset(), id_value(h)?))
            }),
            "find_by_id!" if args.len() == 1 => {
                Some((call.message_loc().unwrap().start_offset(), send_args(&call).into_iter().next().unwrap()))
            }
            "find_by!" => match args.as_slice() {
                [h] => id_value(h).map(|v| (call.message_loc().unwrap().start_offset(), v)),
                _ => None,
            },
            _ => None,
        };
        let Some((s, id)) = found else { return };
        let good = format!("find({})", node_text(&id));
        let msg = format!("Use `{good}` instead of `{}`.", cx.src.slice(s, end));
        cx.add_fix(s, end, msg, |c| c.replace(s, end, &good));
    }
}
