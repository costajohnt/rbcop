use crate::prelude::*;

pub const NAME: &str = "Style/For";
pub static COP: For = For;
pub struct For;

/// ForToEachCorrector#requires_parentheses?
fn requires_parentheses(node: &Node) -> bool {
    match node {
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            !is_csend(&c)
                && c.block().is_none_or(|b| b.as_block_node().is_none())
                && helpers::node_types::is_operator_method(method_name(&c))
        }
        Node::RangeNode { .. } | Node::AndNode { .. } | Node::OrNode { .. } => true,
        _ => false,
    }
}

impl Cop for For {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::FOR_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let each_style = cx.cfg.str("EnforcedStyle") != Some("for");
        if let Some(f) = node.as_for_node() {
            if !each_style {
                return;
            }
            let coll = f.collection();
            let coll_src = node_text(&coll);
            let coll_src = if requires_parentheses(&coll) { format!("({coll_src})") } else { coll_src.to_string() };
            let dot = if coll.as_call_node().is_some_and(|c| is_csend(&c)) { "&." } else { "." };
            let correction = format!("{coll_src}{dot}each do |{}|", node_text(&f.index()));
            let loc = node.location();
            let (s, e) = (loc.start_offset(), loc.end_offset());
            let end = f.do_keyword_loc().map_or(coll.location().end_offset(), |d| d.end_offset());
            cx.add_fix(s, e, "Prefer `each` over `for`.", |c| c.replace(s, end, &correction));
            return;
        }
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let src = cx.src;
        let (open, close) = (block.opening_loc(), block.closing_loc());
        if src.line_of(open.start_offset()) == src.line_of(close.start_offset())
            || method_name(&call) != "each"
            || call.arguments().is_some()
            || each_style
        {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        if block.body().is_some_and(|b| b.as_begin_node().is_some_and(|b| b.begin_keyword_loc().is_none())) {
            return;
        }
        // Regular block arguments (numbered / `it` parameters count as none).
        let params = block.parameters().and_then(|p| p.as_block_parameters_node());
        let vars: Vec<Node> = params.as_ref().map(helpers::block_args::children).unwrap_or_default();
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        let (end, correction) = if vars.is_empty() {
            (open.end_offset(), format!("for _ in {} do", node_text(&recv)))
        } else {
            let names: Vec<&str> = vars.iter().map(node_text).collect();
            let p = params.unwrap().location().end_offset();
            (p, format!("for {} in {} do", names.join(", "), node_text(&recv)))
        };
        cx.add_fix(s, e, "Prefer `for` over `each`.", |c| c.replace(s, end, &correction));
    }
}
