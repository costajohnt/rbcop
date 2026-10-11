use crate::helpers::node_types::is_binary_operation;
use crate::helpers::parser_parent::{is_operator_keyword, is_ternary, parser_parent};
use crate::prelude::*;

pub const NAME: &str = "Style/NonNilCheck";
pub static COP: NonNilCheck = NonNilCheck;
pub struct NonNilCheck;

/// Ranges of nodes `ignore_node`d by `on_def` (the last expression of a predicate method).
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl Cop for NonNilCheck {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if let Some(def) = node.as_def_node() {
            if def.name().as_slice().ends_with(b"?")
                && let Some(last) = def.body().and_then(ignored_last)
            {
                let loc = last.location();
                cx.state::<Ignored>().0.push((loc.start_offset(), loc.end_offset()));
            }
            return;
        }
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let semantic = cx.cfg.bool("IncludeSemanticChanges").unwrap_or(false);
        let (s, e) = send_range(&call);
        if cx.state::<Ignored>().0.contains(&(s, e)) {
            return;
        }
        if !semantic {
            let nc = cx.config.for_cop("Style/NilComparison");
            if nc.truthy("Enabled") && nc.str("EnforcedStyle") == Some("comparison") {
                return;
            }
        }
        let args = send_args(&call);
        let name = method_name(&call);
        if name == "!=" {
            // (send _ :!= nil)
            let Some(recv) = call.receiver() else { return };
            if !matches!(args.as_slice(), [a] if a.as_nil_node().is_some()) {
                return;
            }
            let replacement = if semantic {
                node_text(&recv).to_string()
            } else {
                let src = node_text(&recv);
                if operator_expression(&recv) { format!("!({src}).nil?") } else { format!("!{src}.nil?") }
            };
            let msg = if semantic {
                "Explicit non-nil checks are usually redundant.".to_string()
            } else {
                format!("Prefer `{replacement}` over `{}`.", cx.src.slice(s, e))
            };
            return cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
        }
        if !semantic || !args.is_empty() {
            return;
        }
        if name == "!" {
            // (send (send _ :nil?) :!)
            let Some(inner) = call.receiver().and_then(|r| r.as_call_node()) else { return };
            if is_csend(&inner) || method_name(&inner) != "nil?" || !send_args(&inner).is_empty() {
                return;
            }
            let fix = inner.receiver().map_or("self", |r| node_text(&r)).to_string();
            return cx.add_fix(s, e, "Explicit non-nil checks are usually redundant.", |c| c.replace(s, e, &fix));
        }
        if name == "nil?" {
            // unless_and_nil_check?: the condition of an `unless` (an `if` node; a pattern guard
            // `in pat unless cond` is a parser `unless_guard`, which never matches, #311)
            let Some(parent_node) = parser_parent(cx.ancestors) else { return };
            let Some(parent) = parent_node.as_unless_node() else { return };
            if parent.predicate().location().start_offset() != s || parent.predicate().location().end_offset() != e {
                return;
            }
            let pi = cx.ancestors.iter().position(|a| std::ptr::eq(a, parent_node)).unwrap();
            if is_pattern_guard(parent_node, &cx.ancestors[..pi]) {
                return;
            }
            // autocorrect_unless_nil raises on a receiver-less `nil?`: the offense is lost.
            let Some(recv) = call.receiver() else { return };
            let kw = parent.keyword_loc();
            let fix = node_text(&recv).to_string();
            cx.add_fix(s, e, "Explicit non-nil checks are usually redundant.", |c| {
                c.replace(kw.start_offset(), kw.end_offset(), "if");
                c.replace(s, e, &fix);
            });
        }
    }
}

/// `on_def`: the node `ignore_node`d for a predicate method body (`begin` body: its last child).
fn ignored_last(body: Node<'_>) -> Option<Node<'_>> {
    let Some(stmts) = body.as_statements_node() else { return Some(body) };
    let stmts: Vec<_> = stmts.body().iter().collect();
    if stmts.len() != 1 {
        return stmts.into_iter().last();
    }
    let only = stmts.into_iter().next()?;
    match only.as_parentheses_node() {
        Some(p) => match p.body()?.as_statements_node() {
            Some(s) => s.body().iter().last(),
            None => p.body(),
        },
        None => Some(only),
    }
}

/// NonNilCheck#operator_expression?
fn operator_expression(node: &Node) -> bool {
    is_operator_keyword(node)
        || node.as_call_node().is_some_and(|c| !is_csend(&c) && is_binary_operation(&c))
        || is_ternary(node)
        || matches!(kind::of(node), kind::RANGE_NODE | kind::FLIP_FLOP_NODE)
        || helpers::node_types::is_assignment(node)
}
