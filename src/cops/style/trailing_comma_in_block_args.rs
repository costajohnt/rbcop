use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/TrailingCommaInBlockArgs";
pub static COP: TrailingCommaInBlockArgs = TrailingCommaInBlockArgs;
pub struct TrailingCommaInBlockArgs;

/// `each_descendant(:arg, :optarg, :kwoptarg)` count.
#[derive(Default)]
struct ArgCount(usize);

impl<'pr> Visit<'pr> for ArgCount {
    fn visit_required_parameter_node(&mut self, _: &ruby_prism::RequiredParameterNode<'pr>) {
        self.0 += 1;
    }
    fn visit_optional_parameter_node(&mut self, node: &ruby_prism::OptionalParameterNode<'pr>) {
        self.0 += 1;
        ruby_prism::visit_optional_parameter_node(self, node);
    }
    fn visit_optional_keyword_parameter_node(&mut self, node: &ruby_prism::OptionalKeywordParameterNode<'pr>) {
        self.0 += 1;
        ruby_prism::visit_optional_keyword_parameter_node(self, node);
    }
    // Blocks and lambdas in default values have their own arguments, which parser also counts.
}

impl Cop for TrailingCommaInBlockArgs {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let block = node.as_block_node().unwrap();
        let Some(bp) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        let Some(close) = bp.closing_loc() else { return };
        if close.as_slice() != b"|" {
            return;
        }
        let mut count = ArgCount::default();
        count.visit(&bp.as_node());
        if count.0 <= 1 {
            return;
        }
        let bytes = cx.src.text.as_bytes();
        let mut i = close.start_offset();
        while i > 0 && bytes[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        if i == 0 || bytes[i - 1] != b',' {
            return;
        }
        let (start, end) = (i - 1, i);
        cx.add_fix(start, end, "Useless trailing comma present in block arguments.", |c| c.replace(start, end, ""));
    }
}
