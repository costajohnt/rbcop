use crate::prelude::*;

pub const NAME: &str = "Gemspec/RubyVersionGlobalsUsage";
pub static COP: RubyVersionGlobalsUsage = RubyVersionGlobalsUsage;
pub struct RubyVersionGlobalsUsage;

/// `(const {cbase nil?} name)`
fn top_const(node: &Node, name: &[u8]) -> bool {
    node.as_constant_read_node().is_some_and(|c| c.name().as_slice() == name)
        || node.as_constant_path_node().is_some_and(|c| c.parent().is_none() && c.name().map(|n| n.as_slice()) == Some(name))
}

impl Cop for RubyVersionGlobalsUsage {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CONSTANT_READ_NODE, kind::CONSTANT_PATH_NODE]
    }

    // RuboCop guards with `gem_specification(processed_source.ast)`, a node search called without a
    // block: it returns an Enumerator, which is always truthy, so every gemspec is checked.
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let hit = top_const(node, b"RUBY_VERSION")
            || node.as_constant_path_node().is_some_and(|c| {
                c.name().map(|n| n.as_slice()) == Some(b"VERSION") && c.parent().is_some_and(|p| top_const(&p, b"Ruby"))
            });
        if hit {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), format!("Do not use `{}` in gemspec file.", node_text(node)));
        }
    }
}
