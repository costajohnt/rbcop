use crate::helpers::node_types::is_assignment;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Style/AndOr";
pub static COP: AndOr = AndOr;
pub struct AndOr;

/// `and`/`or` keyword operator of an AndNode/OrNode: (start, end, current, preferred).
fn keyword_operator(node: &Node) -> Option<(usize, usize, &'static str, &'static str)> {
    let op = node.as_and_node().map(|n| n.operator_loc()).or_else(|| node.as_or_node().map(|n| n.operator_loc()))?;
    let (cur, alt) = match op.as_slice() {
        b"and" => ("and", "&&"),
        b"or" => ("or", "||"),
        _ => return None,
    };
    Some((op.start_offset(), op.end_offset(), cur, alt))
}

/// A corrector call recorded before `add_fix` (the block cannot see the AST).
enum Edit {
    Remove(usize, usize),
    InsertBefore(usize, usize, &'static str),
    InsertAfter(usize, usize, &'static str),
    Wrap(usize, usize),
    Replace(usize, usize, &'static str),
}

/// `process_logical_operator`: `parent_is_and` is `node.parent&.and_type?`.
fn report(cx: &mut Cx, node: &Node, parent_is_and: bool) {
    let Some((s, e, cur, alt)) = keyword_operator(node) else { return };
    let src = cx.src.text;
    let (lhs, rhs) = match node.as_and_node() {
        Some(n) => (n.left(), n.right()),
        None => {
            let n = node.as_or_node().unwrap();
            (n.left(), n.right())
        }
    };
    let mut edits = Vec::new();
    for (expr, is_last) in [(&lhs, false), (&rhs, true)] {
        match expr.as_call_node().filter(|c| !is_csend(c) && !has_block(c)) {
            Some(call) => correct_send(&call, expr, is_last, src, &mut edits),
            None if matches!(kind::of(expr), kind::RETURN_NODE | kind::NEXT_NODE | kind::BREAK_NODE | kind::YIELD_NODE)
                || is_assignment(expr) =>
            {
                correct_other(expr, is_last, &mut edits)
            }
            None => {}
        }
    }
    edits.push(Edit::Replace(s, e, alt));
    // keep_operator_precedence
    if node.as_or_node().is_some() && parent_is_and {
        let l = node.location();
        edits.push(Edit::Wrap(l.start_offset(), l.end_offset()));
    } else if node.as_and_node().is_some() && rhs.as_or_node().is_some() {
        let l = rhs.location();
        edits.push(Edit::Wrap(l.start_offset(), l.end_offset()));
    }
    cx.add_fix(s, e, format!("Use `{alt}` instead of `{cur}`."), |c| {
        for edit in edits {
            match edit {
                Edit::Remove(a, b) => c.remove(a, b),
                Edit::InsertBefore(a, b, t) => c.insert_before(a, b, t),
                Edit::InsertAfter(a, b, t) => c.insert_after(a, b, t),
                Edit::Wrap(a, b) => c.wrap(a, b, "(", ")"),
                Edit::Replace(a, b, t) => c.replace(a, b, t),
            }
        }
    });
}

/// A call with a `{}`/`do` block is a parser `block`, not a `send`.
fn has_block(call: &ruby_prism::CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_node().is_some())
}

/// Parser `last_argument`: a `&blk` block pass counts as an argument.
fn last_argument(call: &ruby_prism::CallNode) -> Option<(usize, usize)> {
    let block_pass = call.block().filter(|b| b.as_block_argument_node().is_some());
    let last = block_pass.or_else(|| call.arguments().and_then(|a| a.arguments().iter().last()))?;
    Some((last.location().start_offset(), last.location().end_offset()))
}

fn correct_send(call: &ruby_prism::CallNode, node: &Node, is_last: bool, src: &str, edits: &mut Vec<Edit>) {
    let name = method_name(call);
    if name == "!" {
        // correct_not
        match call.message_loc().map(|l| l.as_slice()) {
            Some(b"!") => {
                if let Some(recv) = call.receiver()
                    && let Some(rc) = recv.as_call_node().filter(|c| !is_csend(c) && !has_block(c))
                {
                    correct_send(&rc, &recv, false, src, edits);
                }
            }
            Some(b"not") => correct_other(node, is_last, edits),
            _ => {}
        }
        return;
    }
    if call.equal_loc().is_some() {
        // correct_setter
        if let (Some(recv), Some(last)) = (call.receiver(), last_argument(call)) {
            let r = recv.location();
            edits.push(Edit::InsertBefore(r.start_offset(), r.end_offset(), "("));
            edits.push(Edit::InsertAfter(last.0, last.1, ")"));
        }
        return;
    }
    if matches!(name, "==" | "===" | "!=" | "<=" | ">=" | ">" | "<") {
        return correct_other(node, is_last, edits);
    }
    // correctable_send?
    let parenthesized = call.closing_loc().is_some_and(|l| l.as_slice() == b")");
    let Some(last) = last_argument(call) else { return };
    if parenthesized || name == "[]" {
        return;
    }
    let Some(sel) = call.message_loc() else { return };
    // whitespace_before_arg
    let begin = sel.end_offset();
    let (s, e) = send_range(call);
    let end = if QUESTION_NON_SPACE.is_match(&src[s..e]) { begin } else { begin + 1 };
    edits.push(Edit::Remove(begin, end));
    edits.push(Edit::InsertBefore(begin, end, "("));
    edits.push(Edit::InsertAfter(last.0, last.1, ")"));
}

static QUESTION_NON_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\?[^ \t\n\x0B\x0C\r]").unwrap());

fn correct_other(node: &Node, is_last: bool, edits: &mut Vec<Edit>) {
    // parenthesized_call?
    let open = match kind::of(node) {
        kind::CALL_NODE => node.as_call_node().unwrap().opening_loc(),
        kind::YIELD_NODE => node.as_yield_node().unwrap().lparen_loc(),
        _ => None,
    };
    if open.is_some_and(|l| l.as_slice() == b"(") {
        return;
    }
    // `node.children.empty?` and it is the last child of the and/or
    let childless = match kind::of(node) {
        kind::RETURN_NODE => node.as_return_node().unwrap().arguments().is_none(),
        kind::NEXT_NODE => node.as_next_node().unwrap().arguments().is_none(),
        kind::BREAK_NODE => node.as_break_node().unwrap().arguments().is_none(),
        kind::YIELD_NODE => node.as_yield_node().unwrap().arguments().is_none(),
        _ => false,
    };
    if childless && is_last {
        return;
    }
    let (s, e) = match node.as_call_node() {
        Some(c) => send_range(&c),
        None => (node.location().start_offset(), node.location().end_offset()),
    };
    edits.push(Edit::Wrap(s, e));
}

/// Collects every and/or node of a subtree (`each_node`) with whether its parent is an `and`.
struct Collect<'pr> {
    found: Vec<(Node<'pr>, bool)>,
    stack: Vec<bool>,
}

impl<'pr> Visit<'pr> for Collect<'pr> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let is_and = node.as_and_node().is_some();
        if is_and || node.as_or_node().is_some() {
            let parent_is_and = self.stack.last().copied().unwrap_or(false);
            self.found.push((node, parent_is_and));
        }
        self.stack.push(is_and);
    }

    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }
}

impl Cop for AndOr {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::AND_NODE, kind::OR_NODE, kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let always = cx.cfg.str("EnforcedStyle") == Some("always");
        let predicate = if let Some(n) = node.as_if_node() {
            n.predicate()
        } else if let Some(n) = node.as_unless_node() {
            n.predicate()
        } else if let Some(n) = node.as_while_node() {
            n.predicate()
        } else if let Some(n) = node.as_until_node() {
            n.predicate()
        } else {
            if always {
                let parent_is_and = cx.parent().is_some_and(|p| p.as_and_node().is_some());
                report(cx, node, parent_is_and);
            }
            return;
        };
        if always || is_pattern_guard(node, cx.ancestors) {
            return;
        }
        // The predicate's own parent is the conditional, never an `and`.
        let mut c = Collect { found: Vec::new(), stack: vec![false] };
        c.visit(&predicate);
        for (n, parent_is_and) in &c.found {
            report(cx, n, *parent_is_and);
        }
    }
}
