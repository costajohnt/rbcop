use crate::prelude::*;

pub const NAME: &str = "Lint/ToJSON";
pub static COP: ToJSON = ToJSON;
pub struct ToJSON;

impl Cop for ToJSON {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.name().as_slice() != b"to_json" || def.parameters().is_some() {
            return;
        }
        let l = node.location();
        let lparen = def.lparen_loc().map(|p| (p.start_offset(), p.end_offset()));
        let name = def.name_loc();
        let (ns, ne) = (name.start_offset(), name.end_offset());
        cx.add_fix(l.start_offset(), l.end_offset(), "`#to_json` requires an optional argument to be parsable via JSON.generate(obj).", |c| match lparen {
            Some((ps, pe)) => c.insert_after(ps, pe, "*_args"),
            None => c.insert_after(ns, ne, "(*_args)"),
        });
    }
}
