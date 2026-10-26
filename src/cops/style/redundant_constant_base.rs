use crate::prelude::*;

pub const NAME: &str = "Style/RedundantConstantBase";
pub static COP: RedundantConstantBase = RedundantConstantBase;
pub struct RedundantConstantBase;

impl Cop for RedundantConstantBase {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_PATH_NODE, kind::CONSTANT_PATH_TARGET_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // `::Foo`: the cbase is the `::` of a path without a parent.
        let delim = match node {
            Node::ConstantPathNode { .. } => {
                let p = node.as_constant_path_node().unwrap();
                if p.parent().is_some() {
                    return;
                }
                p.delimiter_loc()
            }
            _ => {
                let p = node.as_constant_path_target_node().unwrap();
                if p.parent().is_some() {
                    return;
                }
                p.delimiter_loc()
            }
        };
        if cx.config.for_cop("Lint/ConstantResolution").truthy("Enabled") {
            return;
        }
        let off = delim.start_offset();
        // module_nesting_ancestors_of(node).none?: class/module ancestors, minus classes whose
        // superclass contains the node.
        let nesting: Vec<&Node> = cx
            .ancestors
            .iter()
            .filter(|a| match a {
                Node::ModuleNode { .. } => true,
                Node::ClassNode { .. } => !a.as_class_node().unwrap().superclass().is_some_and(|s| {
                    let l = s.location();
                    l.start_offset() <= off && off < l.end_offset()
                }),
                _ => false,
            })
            .collect();
        if !nesting.is_empty() && !provably_unshadowed(cx, node, &nesting) {
            return;
        }
        let (start, end) = (off, delim.end_offset());
        cx.add_fix(start, end, "Remove redundant `::`.", |c| c.remove(start, end));
    }
}

/// `provably_unshadowed?`: with the project index, the constant resolves to the same declaration
/// through the module nesting as from the top level.
fn provably_unshadowed(cx: &Cx, node: &Node, nesting: &[&Node]) -> bool {
    let Some(index) = crate::project_index::get() else { return false };
    // first_constant_segment: the cbase's parent is a `const` (not a casgn or mlhs target).
    let Some(path) = node.as_constant_path_node() else { return false };
    let start = node.location().start_offset();
    let target = cx.parent().is_some_and(|p| match p {
        Node::ConstantPathWriteNode { .. } => p.as_constant_path_write_node().unwrap().target().location().start_offset() == start,
        Node::ConstantPathOrWriteNode { .. } => p.as_constant_path_or_write_node().unwrap().target().location().start_offset() == start,
        Node::ConstantPathAndWriteNode { .. } => p.as_constant_path_and_write_node().unwrap().target().location().start_offset() == start,
        Node::ConstantPathOperatorWriteNode { .. } => {
            p.as_constant_path_operator_write_node().unwrap().target().location().start_offset() == start
        }
        _ => false,
    });
    let Some(segment) = path.name().filter(|_| !target) else { return false };
    let segment = String::from_utf8_lossy(segment.as_slice());
    // A nil const_name in the nesting raises (TypeError), rescued as false.
    let Some(names) = nesting
        .iter()
        .map(|a| crate::helpers::const_name::const_name(&crate::project_index::identifier(a)?))
        .collect::<Option<Vec<String>>>()
    else {
        return false;
    };
    let relative = index.resolve_constant(&segment, &names);
    let absolute = index.resolve_constant(&segment, &[]);
    let ix = index.read();
    matches!((relative, absolute), (Some(r), Some(a)) if ix.name(r) == ix.name(a))
}
