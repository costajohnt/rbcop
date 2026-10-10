use crate::helpers::parser_parent::is_ternary;
use crate::helpers::range_help::{Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use ruby_prism::Visit;
use std::cell::Cell;

pub const NAME: &str = "Style/NestedTernaryOperator";
pub static COP: NestedTernaryOperator = NestedTernaryOperator;
pub struct NestedTernaryOperator;

const MSG: &str = "Ternary operators must not be nested. Prefer `if` or `else` constructs instead.";

/// `ignore_node`d outer ternaries.
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl Cop for NestedTernaryOperator {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(if_node) = node.as_if_node().filter(|_| is_ternary(node)) else { return };
        let mut nested = Ternaries(Vec::new());
        ruby_prism::visit_if_node(&mut nested, &if_node);
        if nested.0.is_empty() {
            return;
        }
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let ws = Surrounding { whitespace: true, ..Default::default() };
        let q = if_node.then_keyword_loc().unwrap();
        let q = range_with_surrounding_space(cx.src.text, q.start_offset(), q.end_offset(), ws);
        let colon = if_node.subsequent().and_then(|c| c.as_else_node()).map(|c| c.else_keyword_loc());
        let colon = colon.map(|c| range_with_surrounding_space(cx.src.text, c.start_offset(), c.end_offset(), ws));
        let branch = if_node.statements().and_then(|s| s.body().iter().next()).map(|b| {
            let src = node_text(&b);
            let stripped = src.strip_prefix('(').and_then(|x| x.strip_suffix(')')).unwrap_or(src);
            (b.location().start_offset(), b.location().end_offset(), stripped.to_string())
        });
        for (ns, ne) in nested.0 {
            let ignored = cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= s && e <= b);
            let ran = Cell::new(false);
            cx.add_fix(ns, ne, MSG, |c| {
                if ignored {
                    return;
                }
                c.replace(q.0, q.1, "\n");
                if let Some(colon) = colon {
                    c.replace(colon.0, colon.1, "\nelse\n");
                }
                if let Some((bs, be, src)) = &branch {
                    c.replace(*bs, *be, src);
                }
                c.wrap(s, e, "if ", "\nend");
                ran.set(true);
            });
            if ran.get() {
                cx.state::<Ignored>().0.push((s, e));
            }
        }
    }
}

/// Ternary descendants, pre-order.
struct Ternaries(Vec<(usize, usize)>);

impl<'pr> Visit<'pr> for Ternaries {
    fn visit_if_node(&mut self, node: &ruby_prism::IfNode<'pr>) {
        if node.if_keyword_loc().is_none() {
            let loc = node.location();
            self.0.push((loc.start_offset(), loc.end_offset()));
        }
        ruby_prism::visit_if_node(self, node);
    }
}
