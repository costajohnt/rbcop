use crate::prelude::*;

pub const NAME: &str = "Style/ClassMethods";
pub static COP: ClassMethods = ClassMethods;
pub struct ClassMethods;

impl Cop for ClassMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let (name, body) = match node {
            Node::ClassNode { .. } => {
                let c = node.as_class_node().unwrap();
                (c.constant_path(), c.body())
            }
            _ => {
                let m = node.as_module_node().unwrap();
                (m.constant_path(), m.body())
            }
        };
        // Only a plain statements body is a parser `defs` / `begin` (a rescue body is `rescue`).
        let Some(stmts) = body.and_then(|b| b.as_statements_node()) else { return };
        let name_range = (name.location().start_offset(), name.location().end_offset());
        for stmt in stmts.body().iter() {
            let Some(def) = stmt.as_def_node() else { continue };
            let Some(recv) = def.receiver() else { continue };
            let (s, e) = (recv.location().start_offset(), recv.location().end_offset());
            if !helpers::node_equality::same(cx.src, name_range, (s, e)) {
                continue;
            }
            let loc = match recv.as_constant_path_node() {
                Some(p) => p.name_loc(),
                None => recv.location(),
            };
            let msg = format!(
                "Use `self.{}` instead of `{}.{}`.",
                String::from_utf8_lossy(def.name().as_slice()),
                node_text(&name),
                String::from_utf8_lossy(def.name().as_slice())
            );
            cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| c.replace(s, e, "self"));
        }
    }
}
