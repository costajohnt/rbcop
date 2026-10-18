use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/DescribedClassModuleWrapping";
pub static COP: DescribedClassModuleWrapping = DescribedClassModuleWrapping;
pub struct DescribedClassModuleWrapping;

impl Cop for DescribedClassModuleWrapping {
    fn kinds(&self) -> &'static [usize] {
        &[kind::MODULE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // def_node_search (any_block (send #explicit_rspec? #ExampleGroups.all ...) ...)
        struct V<'a> {
            l: &'a helpers::rspec::Language,
            found: bool,
        }
        impl<'pr> Visit<'pr> for V<'_> {
            fn visit_call_node(&mut self, n: &ruby_prism::CallNode<'pr>) {
                // the search stops at the first match
                if self.found {
                    return;
                }
                if helpers::rspec::block_of(n).is_some()
                    && n.receiver().is_some_and(|r| helpers::rspec::is_explicit_rspec(&r))
                    && self.l.example_group(method_name(n))
                {
                    self.found = true;
                }
                if !self.found {
                    ruby_prism::visit_call_node(self, n);
                }
            }
        }
        let l = helpers::rspec::lang(cx);
        let mut v = V { l: &l, found: false };
        v.visit(node);
        if v.found {
            let loc = node.location();
            cx.add(loc.start_offset(), loc.end_offset(), "Avoid opening modules and defining specs within them.");
        }
    }
}
