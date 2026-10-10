use crate::prelude::*;

pub const NAME: &str = "Style/GlobalStdStream";
pub static COP: GlobalStdStream = GlobalStdStream;
pub struct GlobalStdStream;

impl Cop for GlobalStdStream {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let parent = cx.ancestors.last();
        let name = if let Some(path) = node.as_constant_path_node() {
            // `Foo::STDOUT` is namespaced; `::STDOUT` is not. A casgn target is not a const node.
            if path.parent().is_some() {
                return;
            }
            if parent.is_some_and(|p| {
                matches!(
                    p,
                    Node::ConstantPathWriteNode { .. }
                        | Node::ConstantPathOperatorWriteNode { .. }
                        | Node::ConstantPathOrWriteNode { .. }
                        | Node::ConstantPathAndWriteNode { .. }
                )
            }) {
                return;
            }
            text(&path.name_loc())
        } else {
            node_text(node)
        };
        if !matches!(name, "STDIN" | "STDOUT" | "STDERR") {
            return;
        }
        let gvar = format!("${}", name.to_ascii_lowercase());
        // (gvasgn %1 (const nil? _)): `$stdout = STDOUT` is allowed.
        if node.as_constant_read_node().is_some()
            && parent
                .and_then(|p| p.as_global_variable_write_node())
                .is_some_and(|w| w.name().as_slice() == gvar.as_bytes())
        {
            return;
        }
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let msg = format!("Use `{gvar}` instead of `{name}`.");
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &gvar));
    }
}
