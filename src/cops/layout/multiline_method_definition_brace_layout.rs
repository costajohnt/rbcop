use crate::helpers::multiline_literal_brace_layout as mlbl;
use crate::prelude::*;

pub const NAME: &str = "Layout/MultilineMethodDefinitionBraceLayout";
pub static COP: MultilineMethodDefinitionBraceLayout = MultilineMethodDefinitionBraceLayout;
pub struct MultilineMethodDefinitionBraceLayout;

const MSGS: mlbl::Messages = [
    "Closing method definition brace must be on the same line as the last parameter when opening brace is on the same line as the first parameter.",
    "Closing method definition brace must be on the line after the last parameter when opening brace is on a separate line from the first parameter.",
    "Closing method definition brace must be on the line after the last parameter.",
    "Closing method definition brace must be on the same line as the last parameter.",
];

impl Cop for MultilineMethodDefinitionBraceLayout {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let (Some(open), Some(close), Some(params)) = (def.lparen_loc(), def.rparen_loc(), def.parameters()) else {
            return;
        };
        let mut children: Vec<Node> = params.requireds().iter().collect();
        children.extend(params.optionals().iter());
        children.extend(crate::ast::parser_rest(params.rest()));
        children.extend(params.posts().iter());
        children.extend(params.keywords().iter());
        children.extend(params.keyword_rest());
        children.extend(params.block().map(|b| b.as_node()));
        children.sort_by_key(|n| n.location().start_offset());
        // The parser `args` node spans the parentheses.
        let close = (close.start_offset(), close.end_offset());
        mlbl::check(cx, &MSGS, open.start_offset(), close, (open.start_offset(), close.1), &children, false, Ok(None), node);
    }
}
