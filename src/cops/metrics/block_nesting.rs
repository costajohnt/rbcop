use crate::prelude::*;
use ruby_prism::Visit;
use std::cell::Cell;

pub const NAME: &str = "Metrics/BlockNesting";
pub static COP: BlockNesting = BlockNesting;
pub struct BlockNesting;

impl Cop for BlockNesting {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let max = cx.cfg.int("Max").unwrap_or(0);
        let blocks = cx.cfg.bool("CountBlocks").unwrap_or(false);
        let modifiers = cx.cfg.bool("CountModifierForms").unwrap_or(false);
        let mut v = Nesting { cx, max, blocks, modifiers, level: 0, ignored: Vec::new(), guard: None };
        v.visit(node);
    }
}

struct Nesting<'a, 'b, 'c> {
    cx: &'a mut Cx<'b, 'c>,
    max: i64,
    blocks: bool,
    modifiers: bool,
    level: i64,
    ignored: Vec<(usize, usize)>,
    guard: Option<(usize, usize)>,
}

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// Parser range of an `elsif` if-node: from the keyword to the end of its last branch.
fn elsif_end(node: &ruby_prism::IfNode) -> usize {
    match node.subsequent() {
        Some(s) => match s.as_if_node() {
            Some(i) => elsif_end(&i),
            None => {
                let e = s.as_else_node().unwrap();
                e.statements().map_or(e.else_keyword_loc().end_offset(), |s| s.location().end_offset())
            }
        },
        None => node.statements().map_or(node.predicate().location().end_offset(), |s| s.location().end_offset()),
    }
}

impl<'pr> Nesting<'_, '_, '_> {
    /// `check_nesting_level` for a considered node, then its children one level deeper if it counts.
    fn nest(&mut self, range: (usize, usize), counts: bool, walk: impl FnOnce(&mut Self)) {
        let saved = self.level;
        self.level += counts as i64;
        if self.level > self.max && !self.ignored.iter().any(|&(s, e)| s <= range.0 && e >= range.1) {
            let ran = Cell::new(false);
            let msg = format!("Avoid more than {} levels of block nesting.", self.max);
            self.cx.add_fix(range.0, range.1, msg, |_| ran.set(true));
            if ran.get() {
                self.ignored.push(range);
            }
        }
        walk(self);
        self.level = saved;
    }

    fn block_call(&mut self, node: &Node<'pr>, has_block: bool, walk: impl FnOnce(&mut Self)) {
        if self.blocks && has_block {
            self.nest(range(node), true, walk);
        } else {
            walk(self);
        }
    }
}

impl<'pr> Visit<'pr> for Nesting<'_, '_, '_> {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        let r = range(&node.as_node());
        if self.guard == Some(r) {
            return ruby_prism::visit_if_node(self, node);
        }
        let kw = node.if_keyword_loc();
        let elsif = kw.as_ref().is_some_and(|l| l.as_slice() == b"elsif");
        let modifier = kw.is_some() && node.end_keyword_loc().is_none() && !elsif;
        let counts = !elsif && (!modifier || self.modifiers);
        let r = if elsif { (r.0, elsif_end(node)) } else { r };
        self.nest(r, counts, |s| ruby_prism::visit_if_node(s, node));
    }
    fn visit_unless_node(&mut self, node: &ruby_prism::UnlessNode<'pr>) {
        let r = range(&node.as_node());
        if self.guard == Some(r) {
            return ruby_prism::visit_unless_node(self, node);
        }
        let counts = node.end_keyword_loc().is_some() || self.modifiers;
        self.nest(r, counts, |s| ruby_prism::visit_unless_node(s, node));
    }
    fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
        let p = node.pattern();
        let saved = self.guard;
        if matches!(kind::of(&p), kind::IF_NODE | kind::UNLESS_NODE) {
            self.guard = Some(range(&p));
        }
        ruby_prism::visit_in_node(self, node);
        self.guard = saved;
    }
    fn visit_case_node(&mut self, node: &ruby_prism::CaseNode<'pr>) {
        self.nest(range(&node.as_node()), true, |s| ruby_prism::visit_case_node(s, node));
    }
    fn visit_case_match_node(&mut self, node: &ruby_prism::CaseMatchNode<'pr>) {
        self.nest(range(&node.as_node()), true, |s| ruby_prism::visit_case_match_node(s, node));
    }
    fn visit_while_node(&mut self, node: &ruby_prism::WhileNode<'pr>) {
        self.nest(range(&node.as_node()), true, |s| ruby_prism::visit_while_node(s, node));
    }
    fn visit_until_node(&mut self, node: &ruby_prism::UntilNode<'pr>) {
        self.nest(range(&node.as_node()), true, |s| ruby_prism::visit_until_node(s, node));
    }
    fn visit_for_node(&mut self, node: &ruby_prism::ForNode<'pr>) {
        self.nest(range(&node.as_node()), true, |s| ruby_prism::visit_for_node(s, node));
    }
    fn visit_rescue_node(&mut self, node: &ruby_prism::RescueNode<'pr>) {
        let r = helpers::resbody::range(self.cx.src, node);
        // Later clauses are siblings in parser, not nested in this one.
        self.nest(r, true, |s| {
            for e in node.exceptions().iter() {
                s.visit(&e);
            }
            if let Some(v) = node.reference() {
                s.visit(&v);
            }
            if let Some(b) = node.statements() {
                s.visit_statements_node(&b);
            }
        });
        if let Some(next) = node.subsequent() {
            self.visit_rescue_node(&next);
        }
    }
    fn visit_rescue_modifier_node(&mut self, node: &ruby_prism::RescueModifierNode<'pr>) {
        self.visit(&node.expression());
        let r = helpers::resbody::modifier_range(node);
        self.nest(r, true, |s| s.visit(&node.rescue_expression()));
    }
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        let has_block = node.block().is_some_and(|b| b.as_block_node().is_some());
        self.block_call(&node.as_node(), has_block, |s| ruby_prism::visit_call_node(s, node));
    }
    fn visit_super_node(&mut self, node: &ruby_prism::SuperNode<'pr>) {
        let has_block = node.block().is_some_and(|b| b.as_block_node().is_some());
        self.block_call(&node.as_node(), has_block, |s| ruby_prism::visit_super_node(s, node));
    }
    fn visit_forwarding_super_node(&mut self, node: &ruby_prism::ForwardingSuperNode<'pr>) {
        let has_block = node.block().is_some();
        self.block_call(&node.as_node(), has_block, |s| ruby_prism::visit_forwarding_super_node(s, node));
    }
    fn visit_lambda_node(&mut self, node: &ruby_prism::LambdaNode<'pr>) {
        self.block_call(&node.as_node(), true, |s| ruby_prism::visit_lambda_node(s, node));
    }
}
