use crate::prelude::*;
use helpers::parentheses_corrector;

pub const NAME: &str = "Style/ParenthesesAroundCondition";
pub static COP: ParenthesesAroundCondition = ParenthesesAroundCondition;
pub struct ParenthesesAroundCondition;

fn span(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// `modifier_op?`: a modifier if/unless/while/until or a modifier rescue (not a ternary).
fn modifier_op(n: &Node) -> bool {
    if let Some(i) = n.as_if_node() {
        return i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && i.end_keyword_loc().is_none();
    }
    if let Some(u) = n.as_unless_node() {
        return u.end_keyword_loc().is_none();
    }
    if let Some(w) = n.as_while_node() {
        return w.closing_loc().is_none();
    }
    if let Some(u) = n.as_until_node() {
        return u.closing_loc().is_none();
    }
    n.as_rescue_modifier_node().is_some()
}

/// `while (foo do ... end)`: the parentheses are needed.
fn do_block(n: &Node) -> bool {
    let block = match n.as_call_node() {
        Some(c) => c.block().and_then(|b| b.as_block_node()).map(|b| b.opening_loc()),
        None => n.as_lambda_node().map(|l| l.opening_loc()),
    };
    block.is_some_and(|o| o.as_slice() == b"do")
}

/// SafeAssignment#safe_assignment?: `(begin {equals_asgn? #setter_method?})`.
fn safe_assignment(children: &[Node]) -> bool {
    let [only] = children else { return false };
    if let Some(c) = only.as_call_node() {
        return c.equal_loc().is_some();
    }
    matches!(
        kind::of(only),
        kind::LOCAL_VARIABLE_WRITE_NODE
            | kind::INSTANCE_VARIABLE_WRITE_NODE
            | kind::CLASS_VARIABLE_WRITE_NODE
            | kind::GLOBAL_VARIABLE_WRITE_NODE
            | kind::CONSTANT_WRITE_NODE
            | kind::CONSTANT_PATH_WRITE_NODE
            | kind::MULTI_WRITE_NODE
    )
}

impl Cop for ParenthesesAroundCondition {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let (cond, keyword, is_loop) = if let Some(i) = node.as_if_node() {
            let Some(kw) = i.if_keyword_loc() else { return };
            (i.predicate(), text(&kw), false)
        } else if let Some(u) = node.as_unless_node() {
            (u.predicate(), "unless", false)
        } else if let Some(w) = node.as_while_node() {
            if w.is_begin_modifier() {
                return;
            }
            (w.predicate(), "while", true)
        } else {
            let u = node.as_until_node().unwrap();
            if u.is_begin_modifier() {
                return;
            }
            (u.predicate(), "until", true)
        };
        let Some(parens) = cond.as_parentheses_node() else { return };
        let children: Vec<Node> = match parens.body() {
            None => return,
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().collect(),
                None => vec![b],
            },
        };
        let first = &children[0];
        if is_loop && do_block(first) {
            return;
        }
        let src: &str = cx.src.text;
        if let Some(second) = children.get(1)
            && src[first.location().end_offset()..second.location().start_offset()].contains(';')
        {
            return;
        }
        if modifier_op(first) || first.as_multi_write_node().is_some() {
            return;
        }
        // parens_allowed?
        let (cs, ce) = span(&cond);
        let lower = |i: Option<&u8>| i.is_some_and(|b| b.is_ascii_lowercase());
        let before = if cs == 0 { src.as_bytes().last() } else { src.as_bytes().get(cs - 1) };
        if lower(before) || lower(src.as_bytes().get(ce)) {
            return;
        }
        if safe_assignment(&children) {
            let lint = cx.config.for_cop("Lint/AssignmentInCondition");
            let lint_enabled = lint.truthy("Enabled");
            if cx.cfg.bool("AllowSafeAssignment") == Some(true)
                || (lint_enabled && lint.get("AllowSafeAssignment") == Some(&Value::Bool(true)))
            {
                return;
            }
        }
        if cx.cfg.bool("AllowInMultilineConditions") == Some(true) && cx.src.line_of(cs) != cx.src.line_of(ce) {
            return;
        }
        let article = if keyword == "while" { "a" } else { "an" };
        let msg = format!("Don't use parentheses around the condition of {article} `{keyword}`.");
        let last = children.last();
        let heredoc = children.last().filter(|n| is_heredoc(n)).map(span);
        let (o, c) = (parens.opening_loc(), parens.closing_loc());
        let fix = parentheses_corrector::plan(
            cx.src,
            (o.start_offset(), o.end_offset()),
            (c.start_offset(), c.end_offset()),
            last,
            heredoc,
            None,
        );
        cx.add_fix(cs, ce, msg, |cr| parentheses_corrector::apply(cr, &fix));
    }
}
