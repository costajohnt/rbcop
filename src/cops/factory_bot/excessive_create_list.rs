use crate::prelude::*;

pub const NAME: &str = "FactoryBot/ExcessiveCreateList";
pub static COP: ExcessiveCreateList = ExcessiveCreateList;
pub struct ExcessiveCreateList;

impl Cop for ExcessiveCreateList {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send #factory_call? :create_list {sym str} $(int _) ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "create_list" {
            return;
        }
        if !helpers::factory_bot::factory_call(cx, call.receiver().as_ref()) {
            return;
        }
        let args = send_args(&call);
        let [name, number, ..] = &args[..] else { return };
        if !(helpers::rspec::is_sym(cx, name) || helpers::rspec::is_str(name)) || number.as_integer_node().is_none() {
            return;
        }
        // `number_node.value <= max_amount`, `%<max_amount>s` in the message
        let Some(max) = cx.cfg.num("MaxAmount") else { return };
        let Some(value) = int_literal_value(number) else { return };
        if value as f64 <= max.f64() {
            return;
        }
        let (s, e) = helpers::rspec::node_range(number);
        cx.add(s, e, format!("Avoid using `create_list` with more than {max} items."));
    }
}
