use crate::prelude::*;
use ruby_prism::{InterpolatedStringNode, InterpolatedXStringNode, Location, StringNode, Visit, XStringNode};

pub const NAME: &str = "Style/SingleLineDoEndBlock";
pub static COP: SingleLineDoEndBlock = SingleLineDoEndBlock;
pub struct SingleLineDoEndBlock;

/// Parser `heredoc_end` ranges of the heredocs in a subtree.
#[derive(Default)]
struct Heredocs(Vec<(usize, usize)>);

impl Heredocs {
    fn add(&mut self, node: Node) {
        if let Some((_, end, _)) = helpers::heredoc::parts(&node) {
            self.0.push(end);
        }
    }
}

impl<'pr> Visit<'pr> for Heredocs {
    fn visit_string_node(&mut self, node: &StringNode<'pr>) {
        self.add(node.as_node());
    }
    fn visit_interpolated_string_node(&mut self, node: &InterpolatedStringNode<'pr>) {
        self.add(node.as_node());
        ruby_prism::visit_interpolated_string_node(self, node);
    }
    fn visit_x_string_node(&mut self, node: &XStringNode<'pr>) {
        self.add(node.as_node());
    }
    fn visit_interpolated_x_string_node(&mut self, node: &InterpolatedXStringNode<'pr>) {
        self.add(node.as_node());
        ruby_prism::visit_interpolated_x_string_node(self, node);
    }
}

fn span(l: &Location) -> (usize, usize) {
    (l.start_offset(), l.end_offset())
}

impl Cop for SingleLineDoEndBlock {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE, kind::LAMBDA_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (opening, closing, params, body, lambda)
        let (open, close, params, body, lambda) = if let Some(l) = node.as_lambda_node() {
            (l.opening_loc(), l.closing_loc(), l.parameters(), l.body(), true)
        } else {
            let block = match node {
                Node::CallNode { .. } => node.as_call_node().unwrap().block(),
                Node::SuperNode { .. } => node.as_super_node().unwrap().block(),
                _ => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
            };
            let Some(b) = block.and_then(|b| b.as_block_node()) else { return };
            (b.opening_loc(), b.closing_loc(), b.parameters(), b.body(), false)
        };
        let src = cx.src;
        if open.as_slice() == b"{" || src.line_of(open.start_offset()) != src.line_of(close.start_offset()) {
            return;
        }
        let (start, end) = (node.location().start_offset(), node.location().end_offset());
        // `@config.for_enabled_cop('Layout/RedundantLineBreak')['InspectBlocks']`
        let rlb = cx.config.for_cop("Layout/RedundantLineBreak");
        if rlb.truthy("Enabled")
            && rlb.truthy("InspectBlocks")
            && helpers::check_single_line_suitability::suitable_as_single_line(cx, node, start, end)
        {
            return;
        }
        // `do_line`: `do` for numblock/itblock, empty args or a lambda literal; else the args.
        let do_line = match params.as_ref().and_then(|p| p.as_block_parameters_node()) {
            Some(bp) if !lambda && (bp.parameters().is_some() || bp.locals().iter().next().is_some()) => {
                span(&bp.location())
            }
            _ => span(&open),
        };
        let mut heredocs = Heredocs::default();
        if let Some(b) = &body {
            heredocs.visit(b);
        }
        // `max_by { heredoc_end.line }`: the first heredoc ending on the last such line.
        let heredoc = heredocs.0.iter().copied().fold(None::<(usize, usize)>, |best, h| match best {
            Some(b) if src.line_of(b.0) >= src.line_of(h.0) => Some(b),
            _ => Some(h),
        });
        let close = span(&close);
        cx.add_fix(start, end, "Prefer multiline `do`...`end` block.", |c| {
            c.insert_after(do_line.0, do_line.1, "\n");
            if let Some(h) = heredoc {
                c.remove(close.0, close.1);
                c.insert_after(h.0, h.1, "\nend");
            } else {
                c.insert_before(close.0, close.1, "\n");
            }
        });
    }
}
