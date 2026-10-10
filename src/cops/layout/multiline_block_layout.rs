use crate::prelude::*;
use ruby_prism::BlockParametersNode;

pub const NAME: &str = "Layout/MultilineBlockLayout";
pub static COP: MultilineBlockLayout = MultilineBlockLayout;
pub struct MultilineBlockLayout;

fn arg_string(nodes: Vec<Node>, comma: bool) -> String {
    let mut s = nodes
        .iter()
        .map(|a| match kind::of(a) {
            kind::MULTI_TARGET_NODE => format!("({})", arg_string(helpers::block_args::mlhs_children(a), comma)),
            _ => node_text(a).to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    if comma {
        s.push(',');
    }
    s
}

fn block_arg_string(params: &BlockParametersNode) -> String {
    let comma = helpers::block_args::count_required(params) == 1 && node_text(&params.as_node()).contains(',');
    arg_string(helpers::block_args::children(params), comma)
}

impl Cop for MultilineBlockLayout {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let src = cx.src;
        let (open, params, body, start) = match node.as_block_node() {
            Some(b) => {
                let start = cx.parent().map_or(node.location().start_offset(), |p| p.location().start_offset());
                (b.opening_loc(), b.parameters(), b.body(), start)
            }
            None => {
                let l = node.as_lambda_node().unwrap();
                (l.opening_loc(), l.parameters(), l.body(), node.location().start_offset())
            }
        };
        let end = node.location().end_offset();
        let begin_line = src.line_of(open.start_offset());
        // BlockNode#single_line?: `{`/`do` and `}`/`end` on the same line.
        if begin_line == src.line_of(end) {
            return;
        }
        let params = params.and_then(|p| p.as_block_parameters_node()).filter(helpers::block_args::any);
        if let Some(params) = &params {
            let loc = params.location();
            if begin_line != src.line_of(loc.end_offset()) {
                let necessary = cx.config.cop("Layout/LineLength").enabled && {
                    let max = cx.config.for_cop("Layout/LineLength").int("Max").unwrap_or(120) as usize;
                    let first_line = src.slice(start, end).split_inclusive('\n').next().unwrap_or("");
                    let pipes = if first_line.ends_with("|\n") { 1 } else { 3 };
                    let needed = src.column(start)
                        + pipes
                        + first_line.trim_end_matches(['\n', '\r']).chars().count()
                        + block_arg_string(params).chars().count();
                    needed > max
                };
                if !necessary {
                    cx.add(
                        loc.start_offset(),
                        loc.end_offset(),
                        "Block argument expression is not on the same line as the block start.",
                    );
                }
            }
        }
        if let Some(body) = body {
            let (s, e) = body_range(&body);
            if src.line_of(s) == begin_line {
                cx.add(s, e,"Block body expression is on the same line as the block start.");
            }
        }
    }
}
