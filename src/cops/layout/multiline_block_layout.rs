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
    let comma = helpers::block_args::count_arguments(params) == 1 && node_text(&params.as_node()).contains(',');
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
        // autocorrect(corrector, node): the same full correction for either offense.
        let open_end = open.end_offset();
        let mut args_fix = None;
        let mut expr_before_body = open.start_offset();
        if let Some(p) = params.as_ref().filter(|p| begin_line != src.line_of(p.location().end_offset())) {
            let loc = p.location();
            let o = helpers::range_help::Surrounding {
                side: helpers::range_help::Side::Right,
                newlines: false,
                ..Default::default()
            };
            let (_, end_pos) =
                helpers::range_help::range_with_surrounding_space(src.text, loc.start_offset(), loc.end_offset(), o);
            // ponytail: range_between(begin.end, end_pos) raises when the args precede `{` (lambda): no offense.
            if end_pos < open_end {
                return;
            }
            args_fix = Some((open_end, end_pos, format!(" |{}|", block_arg_string(p))));
            expr_before_body = loc.end_offset();
        }
        let body_fix = body.as_ref().and_then(|b| {
            let (s, e) = body_range(b);
            if src.line_of(s) != src.line_of(expr_before_body) {
                return None;
            }
            // first_node: the first statement of a parser `begin` body not starting with `(`.
            let first = match b.as_statements_node() {
                Some(st) if st.body().len() > 1 => {
                    let f = st.body().iter().next().unwrap();
                    (f.location().start_offset(), f.location().end_offset())
                }
                _ => (s, e),
            };
            Some((first, format!("\n  {}", " ".repeat(src.column(start)))))
        });
        let fix = |c: &mut Corrector| {
            if let Some((a, b, s)) = &args_fix {
                c.replace(*a, *b, s);
            }
            if let Some(((a, b), s)) = &body_fix {
                c.insert_before(*a, *b, s);
            }
        };
        if let Some(params) = &params {
            let loc = params.location();
            if begin_line != src.line_of(loc.end_offset()) {
                let necessary = cx.config.cop("Layout/LineLength").enabled && {
                    let max = helpers::statement_modifier::line_length_max(&cx.config.for_cop("Layout/LineLength"));
                    let first_line = src.slice(start, end).split_inclusive('\n').next().unwrap_or("");
                    let pipes = if first_line.ends_with("|\n") { 1 } else { 3 };
                    let needed = src.column(start)
                        + pipes
                        + first_line.trim_end_matches(['\n', '\r']).chars().count()
                        + block_arg_string(params).chars().count();
                    needed as f64 > max
                };
                if !necessary {
                    let msg = "Block argument expression is not on the same line as the block start.";
                    cx.add_fix(loc.start_offset(), loc.end_offset(), msg, fix);
                }
            }
        }
        if let Some(body) = body {
            let (s, e) = body_range(&body);
            if src.line_of(s) == begin_line {
                cx.add_fix(s, e, "Block body expression is on the same line as the block start.", fix);
            }
        }
    }
}
