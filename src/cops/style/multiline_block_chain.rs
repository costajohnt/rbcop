use crate::prelude::*;
use crate::source::Source;
use ruby_prism::Visit;

pub const NAME: &str = "Style/MultilineBlockChain";
pub static COP: MultilineBlockChain = MultilineBlockChain;
pub struct MultilineBlockChain;

impl Cop for MultilineBlockChain {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE, kind::SUPER_NODE, kind::FORWARDING_SUPER_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_block: the block owner is parser's send_node; walk it without its own block.
        let mut v = Finder { src: cx.src, found: None };
        let send_end = if let Some(call) = node.as_call_node() {
            if !call.block().is_some_and(|b| b.as_block_node().is_some()) {
                return;
            }
            v.check(&call);
            if v.found.is_none()
                && let Some(r) = call.receiver() {
                    v.visit(&r);
                }
            if v.found.is_none()
                && let Some(a) = call.arguments() {
                    v.visit_arguments_node(&a);
                }
            send_range(&call).1
        } else if let Some(s) = node.as_super_node() {
            if !s.block().is_some_and(|b| b.as_block_node().is_some()) {
                return;
            }
            if let Some(a) = s.arguments() {
                v.visit_arguments_node(&a);
            }
            [s.rparen_loc().map(|l| l.end_offset()), s.arguments().map(|a| a.location().end_offset())]
                .into_iter()
                .flatten()
                .max()
                .unwrap_or(s.keyword_loc().end_offset())
        } else {
            // zsuper has no children
            return;
        };
        if let Some(start) = v.found {
            cx.add(start, send_end, "Avoid multi-line chains of blocks.");
        }
    }
}

struct Finder<'a> {
    src: &'a Source<'a>,
    /// `receiver.loc.end.begin_pos` of the first match
    found: Option<usize>,
}

impl Finder<'_> {
    fn check(&mut self, call: &ruby_prism::CallNode) {
        if self.found.is_some() {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // receiver&.any_block_type? && receiver.multiline?
        let (open, close) = if let Some(c) = recv.as_call_node() {
            match c.block().and_then(|b| b.as_block_node()) {
                Some(b) => (b.opening_loc(), b.closing_loc()),
                None => return,
            }
        } else if let Some(l) = recv.as_lambda_node() {
            (l.opening_loc(), l.closing_loc())
        } else {
            return;
        };
        if self.src.line_of(open.start_offset()) != self.src.line_of(close.start_offset()) {
            self.found = Some(close.start_offset());
        }
    }
}

impl<'pr> Visit<'pr> for Finder<'_> {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.check(node);
        if self.found.is_none() {
            ruby_prism::visit_call_node(self, node);
        }
    }
}
