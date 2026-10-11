use crate::helpers::parser_parent::{is_top_const, parser_parent};
use crate::prelude::*;

pub const NAME: &str = "Lint/ConstantResolution";
pub static COP: ConstantResolution = ConstantResolution;
pub struct ConstantResolution;

// ponytail: with AllCops/UseProjectIndex RuboCop only reports ambiguous resolutions (rubydex);
// rbcop has no project index, so this is the index-off behaviour (every unqualified constant).
impl Cop for ConstantResolution {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE]
    }
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let name = String::from_utf8_lossy(node.as_constant_read_node().unwrap().name().as_slice()).into_owned();
        let only = cx.cfg.strings("Only");
        if !(only.is_empty() || only.contains(&name)) || cx.cfg.strings("Ignore").contains(&name) {
            return;
        }
        if defines_module(cx.ancestors) {
            return;
        }
        let loc = node.location();
        let (start, mut end) = (loc.start_offset(), loc.end_offset());
        // The omitted value of `{ Foo: }` is a parser `const` spanning the bare name; Prism's
        // ImplicitNode and the ConstantReadNode inside it include the colon (#315).
        if cx.parent().is_some_and(|p| p.as_implicit_node().is_some()) && cx.src.text.as_bytes()[end - 1] == b':' {
            end -= 1;
        }
        cx.add(start, end, "Fully qualify this constant to avoid possibly ambiguous resolution.");
    }
}

/// `node.parent&.defined_module`: the parent is a class/module (name, superclass or single body
/// statement), or the casgn `Foo::Bar = Class.new` / `Module.new` (call or block) whose scope we are.
fn defines_module(ancestors: &[Node]) -> bool {
    let n = ancestors.len();
    if n >= 2
        && kind::of(&ancestors[n - 1]) == kind::CONSTANT_PATH_NODE
        && let Some(w) = ancestors[n - 2].as_constant_path_write_node()
    {
        return w.value().as_call_node().is_some_and(|c| {
            c.name().as_slice() == b"new"
                && c.receiver().is_some_and(|r| is_top_const(&r, "Class") || is_top_const(&r, "Module"))
        });
    }
    parser_parent(ancestors).is_some_and(|p| matches!(p, Node::ClassNode { .. } | Node::ModuleNode { .. }))
}
