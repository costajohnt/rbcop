use crate::prelude::*;

pub const NAME: &str = "Rails/OrderById";
pub static COP: OrderById = OrderById;
pub struct OrderById;

fn primary_key(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| method_name(&c) == "primary_key" && send_args(&c).is_empty())
}

impl Cop for OrderById {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if method_name(&call) != "order" {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let by_id = helpers::rails::sym_value(arg).as_deref() == Some("id")
            || primary_key(arg)
            || helpers::rails::hash_elements(arg).is_some_and(|pairs| {
                matches!(pairs.as_slice(), [p] if p.as_assoc_node().is_some_and(|a| {
                    let k = a.key();
                    helpers::rails::sym_value(&k).as_deref() == Some("id") || primary_key(&k)
                }))
            });
        if !by_id {
            return;
        }
        let s = call.message_loc().unwrap().start_offset();
        let (_, e) = send_range(&call);
        cx.add(s, e, "Do not use the `id` column for ordering. Use a timestamp column to order chronologically.");
    }
}
