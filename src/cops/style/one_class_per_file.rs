use crate::prelude::*;

pub const NAME: &str = "Style/OneClassPerFile";
pub static COP: OneClassPerFile = OneClassPerFile;
pub struct OneClassPerFile;

const MSG: &str = "Do not define multiple classes/modules at the top level in a single file.";

/// `node.parent.begin_type? ? node.parent.root? : node.root?`
fn top_level(anc: &[Node]) -> bool {
    match anc.len() {
        2 => true,
        // `(class A; end; class B; end)` as the whole file: the parenthesised `begin` is the root.
        4 => {
            anc[2].as_parentheses_node().is_some()
                && anc[1].as_statements_node().is_some_and(|s| s.body().iter().count() == 1)
        }
        _ => false,
    }
}

impl Cop for OneClassPerFile {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !top_level(cx.ancestors) {
            return;
        }
        let path = match node.as_class_node() {
            Some(c) => c.constant_path(),
            None => node.as_module_node().unwrap().constant_path(),
        };
        let short = match (path.as_constant_read_node(), path.as_constant_path_node()) {
            (Some(c), _) => c.name().as_slice(),
            (_, Some(p)) => p.name().map_or(&b""[..], |n| n.as_slice()),
            _ => b"",
        };
        if cx.cfg.array("AllowedClasses").iter().any(|a| a.as_bytes() == short) {
            return;
        }
        let count = cx.state::<usize>();
        *count += 1;
        if *count > 1 {
            cx.add(node.location().start_offset(), path.location().end_offset(), MSG);
        }
    }
}
