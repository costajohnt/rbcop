use crate::helpers::if_node::{Else, is_elsif, parts};
use crate::prelude::*;

pub const NAME: &str = "Style/NegatedIfElseCondition";
pub static COP: NegatedIfElseCondition = NegatedIfElseCondition;
pub struct NegatedIfElseCondition;

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// `node = node.children.first while node&.type?(:begin, :kwbegin)`
fn unwrap_begin(node: Node) -> Option<Node> {
    let mut node = node;
    loop {
        let body = if let Some(p) = node.as_parentheses_node() {
            p.body()
        } else if let Some(b) = node.as_begin_node().filter(|b| b.begin_keyword_loc().is_some()) {
            b.statements().map(|s| s.as_node())
        } else {
            return Some(node);
        };
        node = {
            let s = body?.as_statements_node()?;
            s.body().iter().next()?
        };
    }
}

impl Cop for NegatedIfElseCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) || is_elsif(node) {
            return;
        }
        let (cond, if_body, els) = parts(node);
        // if_else?: an else branch that is not an `elsif`.
        let else_r = match &els {
            Some(Else::Body(Some(b))) => b.range(),
            _ => return,
        };
        let cond_end = cond.location().end_offset();
        let Some(condition) = unwrap_begin(cond) else { return };
        let Some(call) = condition.as_call_node() else { return };
        if is_csend(&call) {
            return;
        }
        let name = method_name(&call);
        let Some(recv) = call.receiver() else { return };
        let negation = name == "!";
        if !negation && !matches!(name, "!=" | "!~") {
            return;
        }
        // double_negation?: `!!x`
        if negation && recv.as_call_node().is_some_and(|r| method_name(&r) == "!" && !is_csend(&r)) {
            return;
        }
        let args = send_args(&call);
        if args.len() >= 2 {
            return;
        }
        let ternary = helpers::parser_parent::is_ternary(node);
        let msg = format!(
            "Invert the negated condition and swap the {} branches.",
            if ternary { "ternary" } else { "if-else" }
        );
        let replacement = if negation {
            node_text(&recv).to_string()
        } else {
            let Some(arg) = args.first() else { return };
            format!("{} {} {}", node_text(&recv), name.replacen('!', "=", 1), node_text(arg))
        };
        let cond_r = range(&condition);
        let src = cx.src;
        // loc.else / loc.end of a non-ternary if/unless.
        let (else_kw, end_kw, then_kw) = if let Some(i) = node.as_if_node() {
            (
                i.subsequent().and_then(|s| s.as_else_node()).map(|e| e.else_keyword_loc()),
                i.end_keyword_loc(),
                i.then_keyword_loc(),
            )
        } else {
            let u = node.as_unless_node().unwrap();
            (u.else_clause().map(|e| e.else_keyword_loc()), u.end_keyword_loc(), u.then_keyword_loc())
        };
        // `if_start = node.loc.begin || node.condition.source_range`
        // (parser's `loc.begin` is also a `;` standing for `then`, which Prism does not record)
        let if_start = then_kw.map_or_else(
            || {
                let rest = &cx.src.text.as_bytes()[cond_end..];
                let ws = rest.iter().take_while(|&&b| matches!(b, b' ' | b'\t')).count();
                if rest.get(ws) == Some(&b';') { cond_end + ws + 1 } else { cond_end }
            },
            |t| t.end_offset(),
        );
        let swap = if ternary {
            let Some(b) = if_body.as_ref() else { return };
            Some((b.range(), else_r))
        } else if if_body.is_none() {
            None
        } else {
            let (Some(e), Some(end)) = (&else_kw, &end_kw) else { return };
            Some(((if_start, e.start_offset()), (e.end_offset(), end.start_offset())))
        };
        let remove_else = else_kw.map(|e| helpers::whole_lines::range_by_whole_lines(src.text, e.start_offset(), e.end_offset(), true));
        let corrected = cx.state::<Vec<(usize, usize)>>().clone();
        let ancestor_corrected = cx.ancestors.iter().any(|a| {
            matches!(kind::of(a), kind::IF_NODE | kind::UNLESS_NODE) && corrected.contains(&range(a))
        });
        let mut ran = false;
        let (s, e) = range(node);
        cx.add_fix(s, e, msg, |c| {
            if ancestor_corrected {
                return;
            }
            c.replace(cond_r.0, cond_r.1, &replacement);
            match swap {
                Some((a, b)) => c.swap(a, b),
                None => {
                    if let Some((rs, re)) = remove_else {
                        c.remove(rs, re);
                    }
                }
            }
            ran = true;
        });
        if ran {
            cx.state::<Vec<(usize, usize)>>().push((s, e));
        }
    }
}
