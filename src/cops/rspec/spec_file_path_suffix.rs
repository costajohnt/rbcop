use crate::prelude::*;

pub const NAME: &str = "RSpec/SpecFilePathSuffix";
pub static COP: SpecFilePathSuffix = SpecFilePathSuffix;
pub struct SpecFilePathSuffix;

impl Cop for SpecFilePathSuffix {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        helpers::rspec::each_top_level_group(&l, node, |_, _, example_group| {
            if example_group && !cx.src.absolute_path().ends_with("_spec.rb") {
                cx.add_global("Spec path should end with `_spec.rb`.");
            }
        });
    }
}
