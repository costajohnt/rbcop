use crate::prelude::*;

pub const NAME: &str = "Rails/AddColumnIndex";
pub static COP: AddColumnIndex = AddColumnIndex;
pub struct AddColumnIndex;

impl Cop for AddColumnIndex {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :add_column $_table $_column <(hash <$(pair {(sym :index) (str "index")} $_) ...>) ...>)
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "add_column") {
            return;
        }
        let args = send_args(&call);
        if args.len() < 3 {
            return;
        }
        let Some(pair) = args[2..].iter().find_map(|a| {
            helpers::rails::hash_elements(a)?.into_iter().find_map(|e| {
                let p = e.as_assoc_node()?;
                let k = p.key();
                (helpers::rails::sym_value(&k).as_deref() == Some("index")
                    || helpers::rails::str_value(&k).as_deref() == Some("index"))
                .then_some(p)
            })
        }) else {
            return;
        };
        let src = cx.src;
        let (ps, pe) = (pair.location().start_offset(), pair.location().end_offset());
        let (rs, re) = helpers::rails::left_space_and_comma(src.text, ps, pe);
        let value = pair.value();
        let mut add_index = format!("add_index {}, {}", node_text(&args[0]), node_text(&args[1]));
        if value.as_hash_node().is_some() {
            let v = node_text(&value);
            add_index += &format!(", {}", helpers::ruby_string::strip(&v[1..v.len() - 1]));
        }
        let (s, e) = send_range(&call);
        let insertion = format!("\n{add_index}");
        cx.add_fix(ps, pe, "`add_column` does not accept an `index` key, use `add_index` instead.", |c| {
            c.remove(rs, re);
            c.insert_after(s, e, &insertion);
        });
    }
}
