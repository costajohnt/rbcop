use crate::prelude::*;
use helpers::regexp_parser::{self as rp, Expr, ExprKind};

pub const NAME: &str = "Lint/RedundantRegexpQuantifiers";
pub static COP: RedundantRegexpQuantifiers = RedundantRegexpQuantifiers;
pub struct RedundantRegexpQuantifiers;

/// `expr.is?(:passive, :group) && expr.one? { |child| child.type != :free_space }`
fn redundant_group(e: &Expr) -> bool {
    e.kind == ExprKind::PassiveGroup && e.children.iter().filter(|c| c.kind != ExprKind::FreeSpace).count() == 1
}

fn mergeable(e: &Expr) -> Option<&'static str> {
    let q = e.quantifier.filter(|q| q.greedy)?;
    match (q.min, q.max) {
        (0, None) => Some("*"),
        (0, Some(1)) => Some("?"),
        (1, None) => Some("+"),
        _ => None,
    }
}

impl Cop for RedundantRegexpQuantifiers {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::REGULAR_EXPRESSION_NODE, kind::MATCH_LAST_LINE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(lit) = rp::literal(node) else { return };
        let Some(tree) = rp::tree(&lit.pattern, lit.extended) else { return };
        let root = Expr { kind: ExprKind::Sequence, start: 0, end: 0, quantifier: None, children: tree };
        let mut all = Vec::new();
        root.each_expression(&mut all);
        let mut seen: Vec<*const Expr> = Vec::new();
        let mut pairs: Vec<(&Expr, &Expr)> = Vec::new();
        for expr in &all {
            if seen.contains(&(*expr as *const Expr)) || !redundant_group(expr) || mergeable(expr).is_none() {
                continue;
            }
            let mut subs = Vec::new();
            expr.each_expression(&mut subs);
            for sub in subs {
                seen.push(sub as *const Expr);
                if !(redundant_group(sub) || sub.kind == ExprKind::Set || sub.terminal()) {
                    break;
                }
                if mergeable(sub).is_some() {
                    pairs.push((expr, sub));
                }
            }
        }
        let src = cx.src.text;
        for (group, child) in pairs {
            let (gq, cq) = (group.quantifier.unwrap(), child.quantifier.unwrap());
            let (q1, q2) = (mergeable(group).unwrap(), mergeable(child).unwrap());
            let replacement = if q1 == q2 { q1 } else { "*" };
            let gtext = &lit.pattern[gq.start..gq.end];
            let ctext = &lit.pattern[cq.start..cq.end];
            let msg = format!("Replace redundant quantifiers `{ctext}` and `{gtext}` with a single `{replacement}`.");
            let off = |b: usize| lit.source_offset(src, b);
            let (gs, ge, cs, ce) = (off(gq.start), off(gq.end), off(cq.start), off(cq.end));
            cx.add_fix(cs, ge, msg, |c| {
                c.replace(gs, ge, "");
                c.replace(cs, ce, replacement);
            });
        }
    }
}
