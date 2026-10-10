use crate::prelude::*;

pub const NAME: &str = "Security/Open";
pub static COP: Open = Open;
pub struct Open;

impl Cop for Open {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "open" {
            return;
        }
        // (send ${nil? (const {nil? cbase} :URI)} :open $_ ...)
        let receiver = match call.receiver() {
            None => "Kernel#".to_string(),
            Some(r) if helpers::const_name::const_name(&r).as_deref() == Some("URI") => format!("{}.", node_text(&r)),
            Some(_) => return,
        };
        let Some(code) = send_args(&call).into_iter().next() else { return };
        if safe(&code) {
            return;
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), format!("The use of `{receiver}open` is a serious security risk."));
    }
}

fn safe(node: &Node) -> bool {
    // A multi-line string is a parser dstr whose first child is its first line: same leading bytes.
    if let Some(s) = node.as_string_node() {
        let v = s.unescaped();
        return !v.is_empty() && !v.starts_with(b"|");
    }
    if node.as_source_file_node().is_some() {
        return true; // ponytail: `__FILE__` is a parser str naming the file; never empty or `|...`
    }
    if let Some(d) = node.as_interpolated_string_node() {
        return d.parts().iter().next().is_some_and(|c| safe(&c));
    }
    if let Some(call) = node.as_call_node() {
        // (send str :+ _): `node.receiver.str_type?`
        if !is_csend(&call) && call.block().is_none() && method_name(&call) == "+"
            && let Some(r) = call.receiver().filter(|r| r.as_string_node().is_some() && !is_parser_dstr(r)) {
                return safe(&r);
            }
    }
    false
}
