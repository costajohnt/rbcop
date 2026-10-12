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
        let groups = helpers::rspec::top_level_groups(&l, node);
        if groups.iter().any(|g| helpers::rspec::example_group(&l, g)) && !cx.src.absolute_path().ends_with("_spec.rb") {
            cx.add_global("Spec path should end with `_spec.rb`.");
        }
    }
}
