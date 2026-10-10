use crate::prelude::*;

pub const NAME: &str = "Lint/BigDecimalNew";
pub static COP: BigDecimalNew = BigDecimalNew;
pub struct BigDecimalNew;

impl Cop for BigDecimalNew {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "new" || is_csend(&call) {
            return;
        }
        // (send (const ${nil? cbase} :BigDecimal) :new ...)
        let Some(recv) = call.receiver() else { return };
        let cbase = match kind::of(&recv) {
            kind::CONSTANT_READ_NODE if recv.as_constant_read_node().unwrap().name().as_slice() == b"BigDecimal" => None,
            kind::CONSTANT_PATH_NODE => {
                let p = recv.as_constant_path_node().unwrap();
                if p.parent().is_some() || p.name().is_none_or(|n| n.as_slice() != b"BigDecimal") {
                    return;
                }
                Some(p.delimiter_loc())
            }
            _ => return,
        };
        let (Some(sel), Some(dot)) = (call.message_loc(), call.call_operator_loc()) else { return };
        let cbase = cbase.map(|l| (l.start_offset(), l.end_offset()));
        let msg = "`BigDecimal.new()` is deprecated. Use `BigDecimal()` instead.";
        cx.add_fix(sel.start_offset(), sel.end_offset(), msg, |c| {
            c.remove(sel.start_offset(), sel.end_offset());
            c.remove(dot.start_offset(), dot.end_offset());
            if let Some((s, e)) = cbase {
                c.remove(s, e);
            }
        });
    }
}
