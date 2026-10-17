use crate::prelude::*;

pub const NAME: &str = "Rails/UnusedIgnoredColumns";
pub static COP: UnusedIgnoredColumns = UnusedIgnoredColumns;
pub struct UnusedIgnoredColumns;

impl Cop for UnusedIgnoredColumns {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::CALL_OPERATOR_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send self :ignored_columns= $array) / (op-asgn (send self :ignored_columns) :+ $array)
        let array = if let Some(c) = helpers::rails::send(node) {
            let args = send_args(&c);
            if method_name(&c) != "ignored_columns=" || !c.receiver().is_some_and(|r| r.as_self_node().is_some()) {
                return;
            }
            let [a] = args.as_slice() else { return };
            let Some(a) = a.as_array_node() else { return };
            a
        } else if let Some(w) = node.as_call_operator_write_node() {
            if is_csend_op(&w)
                || w.read_name().as_slice() != b"ignored_columns"
                || w.binary_operator().as_slice() != b"+"
                || !w.receiver().is_some_and(|r| r.as_self_node().is_some())
            {
                return;
            }
            let Some(a) = w.value().as_array_node() else { return };
            a
        } else {
            return;
        };
        let Some(schema) = helpers::schema::schema() else { return };
        let anc = cx.ancestors;
        let Some(ci) = anc.iter().rposition(|a| a.as_class_node().is_some()) else { return };
        let Some(table) = schema.table_by(&helpers::schema::table_name(anc, ci)) else { return };
        for e in array.elements().iter() {
            // ({str sym} $_)
            let name = if e.as_string_node().is_some() { helpers::rails::str_value(&e) } else { helpers::rails::sym_value(&e) };
            let Some(name) = name else { continue };
            if !table.with_column(&name) {
                let (s, t) = (e.location().start_offset(), e.location().end_offset());
                cx.add(s, t, format!("Remove `{name}` from `ignored_columns` because the column does not exist."));
            }
        }
    }
}

fn is_csend_op(w: &ruby_prism::CallOperatorWriteNode) -> bool {
    w.call_operator_loc().is_some_and(|l| l.as_slice() == b"&.")
}
