use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/TrailingCommaInBlockArgs";
pub static COP: TrailingCommaInBlockArgs = TrailingCommaInBlockArgs;
pub struct TrailingCommaInBlockArgs;

#[derive(Default)]
struct State {
    tokens: Option<Vec<Token>>,
}

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
        let mut count = ArgCount::default();
        count.visit(&bp.as_node());
        if count.0 <= 1 {
            return;
        }
        // argument_tokens: the tokens between the first two tPIPEs within the parser block, whose
        // range starts at the receiver, so a pipe there (`a.select { |t,| }.map { |x, y| }`, or a
        // binary `|`) is found first. Fewer than two pipes, or none between them, raises.
        let start = cx.ancestors.last().map_or(node.location().start_offset(), |p| p.location().start_offset());
        let end = node.location().end_offset();
        let src = cx.src;
        let comma = {
            let st = cx.state::<State>();
            let all = st.tokens.get_or_insert_with(|| helpers::parser_tokens::translated_sorted(src));
            let tokens = helpers::parser_tokens::within(all, start, end);
            let mut pipes = tokens.iter().enumerate().filter(|(_, t)| t.kind == sys::PM_TOKEN_PIPE).map(|(i, _)| i);
            let (Some(b), Some(e)) = (pipes.next(), pipes.next()) else { return };
            let Some(last) = tokens[b + 1..e].last() else { return };
            if last.kind != sys::PM_TOKEN_COMMA {
                return;
            }
            (last.start, last.end)
        };
        let (start, end) = comma;
        cx.add_fix(start, end, "Useless trailing comma present in block arguments.", |c| c.replace(start, end, ""));
    }
}
