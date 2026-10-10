use crate::helpers::node_types::is_operator_method;
use crate::prelude::*;

pub const NAME: &str = "Style/TernaryParentheses";
pub static COP: TernaryParentheses = TernaryParentheses;
pub struct TernaryParentheses;

/// Children of a parser `begin` (parenthesized expression).
fn paren_children<'pr>(node: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    let p = node.as_parentheses_node()?;
    Some(match p.body() {
        None => Vec::new(),
        Some(b) => match b.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![b],
        },
    })
}

/// A call that is a parser send/csend (not a block).
fn plain_call<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// SafeAssignment#safe_assignment?: `(begin {equals_asgn? #setter_method?})`.
fn safe_assignment(cond: &Node) -> bool {
    let Some(children) = paren_children(cond) else { return false };
    let [child] = children.as_slice() else { return false };
    matches!(
        kind::of(child),
        kind::LOCAL_VARIABLE_WRITE_NODE
            | kind::INSTANCE_VARIABLE_WRITE_NODE
            | kind::CLASS_VARIABLE_WRITE_NODE
            | kind::GLOBAL_VARIABLE_WRITE_NODE
            | kind::CONSTANT_WRITE_NODE
            | kind::CONSTANT_PATH_WRITE_NODE
            | kind::MULTI_WRITE_NODE
    ) || plain_call(child).is_some_and(|c| c.equal_loc().is_some())
}

fn complex_condition(cond: &Node) -> bool {
    if let Some(children) = paren_children(cond) {
        return children.iter().any(complex_condition);
    }
    let non_complex = matches!(
        kind::of(cond),
        kind::LOCAL_VARIABLE_READ_NODE
            | kind::IT_LOCAL_VARIABLE_READ_NODE
            | kind::INSTANCE_VARIABLE_READ_NODE
            | kind::GLOBAL_VARIABLE_READ_NODE
            | kind::CLASS_VARIABLE_READ_NODE
            | kind::CONSTANT_READ_NODE
            | kind::CONSTANT_PATH_NODE
            | kind::DEFINED_NODE
            | kind::YIELD_NODE
    ) || plain_call(cond).is_some_and(|c| {
        let name = method_name(&c);
        !is_operator_method(name) || name == "[]"
    });
    !non_complex
}

/// below_ternary_precedence?: English `or` / `and` / `not`.
fn below_ternary_precedence(child: &Node) -> bool {
    if let Some(o) = child.as_or_node() {
        return o.operator_loc().as_slice() == b"or";
    }
    if let Some(a) = child.as_and_node() {
        return a.operator_loc().as_slice() == b"and";
    }
    plain_call(child).is_some_and(|c| method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not"))
}

/// node_args_need_parens? and parenthesize_condition_arguments: (opening range, last argument end).
fn args_fix(node: &Node) -> Option<((usize, usize), usize)> {
    if let Some(d) = node.as_defined_node() {
        if d.rparen_loc().is_some() {
            return None;
        }
        let v = d.value().location();
        return Some(((d.keyword_loc().end_offset(), v.start_offset()), v.end_offset()));
    }
    let c = plain_call(node)?;
    let args = send_args(&c);
    let (first, last) = (args.first()?, args.last()?);
    if c.closing_loc().is_some_and(|l| l.as_slice() == b")") {
        return None;
    }
    let dot = c.call_operator_loc().is_some_and(|l| matches!(l.as_slice(), b"." | b"&."));
    let name = method_name(&c);
    let lettered = name.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic());
    if !(dot || lettered) {
        return None;
    }
    let sel = c.message_loc()?;
    Some(((sel.end_offset(), first.location().start_offset()), last.location().end_offset()))
}

impl Cop for TernaryParentheses {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let n = node.as_if_node().unwrap();
        let cond = n.predicate();
        let cond_src = node_text(&cond);
        if cond_src.split('\n').rfind(|l| !l.is_empty()) == Some(")") {
            return;
        }
        let parens = paren_children(&cond);
        if let Some(first) = parens.as_ref().and_then(|c| c.first()) {
            let pm = if cx.config.target_ruby >= 3.0 {
                first.as_match_predicate_node().is_some()
            } else {
                first.as_match_required_node().is_some()
            };
            if pm {
                return;
            }
        }
        if n.if_keyword_loc().is_some() {
            return;
        }
        // parenthesized_modifier_condition?
        if let Some(first) = parens.as_ref().and_then(|c| c.first())
            && let Some(c) = crate::helpers::if_node::cond(first)
            && c.modifier()
        {
            return;
        }
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("require_no_parentheses");
        let parenthesized = parens.is_some();
        let safe = safe_assignment(&cond);
        let offense = if safe {
            cx.cfg.bool("AllowSafeAssignment") != Some(true)
        } else if style == "require_parentheses_when_complex" {
            if complex_condition(&cond) { !parenthesized } else { parenthesized }
        } else if style == "require_parentheses" {
            !parenthesized
        } else {
            parenthesized
        };
        if !offense {
            return;
        }
        let msg = if style == "require_parentheses_when_complex" {
            let command = if parenthesized { "Only use" } else { "Use" };
            format!("{command} parentheses for ternary expressions with complex conditions.")
        } else {
            let command = if style == "require_parentheses" { "Use" } else { "Omit" };
            format!("{command} parentheses for ternary conditions.")
        };
        let loc = node.location();
        let c = cond.location();
        let (cs, ce) = (c.start_offset(), c.end_offset());
        enum Fix {
            None,
            Unwrap { space: bool, args: Option<((usize, usize), usize)> },
            Wrap,
        }
        let fix = match &parens {
            Some(children) => {
                if safe || children.iter().any(below_ternary_precedence) {
                    Fix::None
                } else {
                    let space = !cx.src.text[ce..].starts_with([' ', '\t', '\n', '\r', '\x0c', '\x0b']);
                    Fix::Unwrap { space, args: children.last().and_then(args_fix) }
                }
            }
            None => Fix::Wrap,
        };
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |corr| match fix {
            Fix::None => {}
            Fix::Wrap => corr.wrap(cs, ce, "(", ")"),
            Fix::Unwrap { space, args } => {
                corr.remove(cs, cs + 1);
                corr.remove(ce - 1, ce);
                if space {
                    corr.insert_after(ce - 1, ce, " ");
                }
                if let Some(((os, oe), last_end)) = args {
                    corr.replace(os, oe, "(");
                    corr.insert_after(last_end, last_end, ")");
                }
            }
        });
    }
}
