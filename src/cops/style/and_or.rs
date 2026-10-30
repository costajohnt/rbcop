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
        correct_operand(expr, is_last, src, &mut edits);
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

/// `/re/ =~ x` with a static regexp is parser's `match_with_lvasgn`, not a `send` (#325).
fn match_with_lvasgn(call: &ruby_prism::CallNode) -> bool {
    call.name().as_slice() == b"=~" && call.receiver().is_some_and(|r| r.as_regular_expression_node().is_some())
}

/// A call with a `{}`/`do` block is a parser `block`, not a `send`.
fn has_block(call: &ruby_prism::CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_node().is_some())
}

/// Parser `last_argument`: a `&blk` block pass counts as an argument.
fn last_argument(args: Option<ruby_prism::ArgumentsNode>, block: Option<Node>) -> Option<(usize, usize)> {
    let block_pass = block.filter(|b| b.as_block_argument_node().is_some());
    let last = block_pass.or_else(|| args.and_then(|a| a.arguments().iter().last()))?;
    Some((last.location().start_offset(), last.location().end_offset()))
}

/// `correct_operand`: a block operand is corrected through its send node (Prism's call or
/// `super` already is that node); `zsuper` and lambdas have nothing to correct.
fn correct_operand(expr: &Node, is_last: bool, src: &str, edits: &mut Vec<Edit>) {
    if let Some(call) = expr.as_call_node().filter(|c| !match_with_lvasgn(c)) {
        correct_send(&call, expr, is_last, src, edits);
    } else if let Some(sup) = expr.as_super_node() {
        correct_super(&sup, src, edits);
    } else if wrap_in_parentheses(expr) {
        correct_other(expr, is_last, edits);
    }
}

/// `wrap_in_parentheses?`: operands that bind looser than `&&` and `||`, or can't be their operands.
fn wrap_in_parentheses(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::RETURN_NODE
            | kind::NEXT_NODE
            | kind::BREAK_NODE
            | kind::YIELD_NODE
            | kind::MATCH_PREDICATE_NODE
            | kind::MATCH_REQUIRED_NODE
            | kind::RANGE_NODE
            | kind::DEFINED_NODE
    ) || is_assignment(node)
        || node.as_if_node().is_some_and(|i| i.if_keyword_loc().is_none())
}

fn correct_send(call: &ruby_prism::CallNode, node: &Node, is_last: bool, src: &str, edits: &mut Vec<Edit>) {
    let name = method_name(call);
    if name == "!" {
        // correct_not
        match call.message_loc().map(|l| l.as_slice()) {
            Some(b"!") => {
                if let Some(recv) = call.receiver() {
                    if let Some(rc) = recv.as_call_node().filter(|c| !match_with_lvasgn(c)) {
                        correct_send(&rc, &recv, false, src, edits);
                    } else if let Some(sup) = recv.as_super_node() {
                        correct_super(&sup, src, edits);
                    }
                }
            }
            Some(b"not") => correct_other(node, is_last, edits),
            _ => {}
        }
        return;
    }
    if call.equal_loc().is_some() {
        // correct_setter
        if let (Some(recv), Some(last)) = (call.receiver(), last_argument(call.arguments(), call.block())) {
            let r = recv.location();
            edits.push(Edit::InsertBefore(r.start_offset(), r.end_offset(), "("));
            edits.push(Edit::InsertAfter(last.0, last.1, ")"));
        }
        return;
    }
    if matches!(name, "==" | "===" | "!=" | "<=" | ">=" | ">" | "<") {
        return correct_other(node, is_last, edits);
    }
    correct_block_receiver(call, src, edits);
    // correctable_send?: operator syntax such as `a + b` or `a[b]` already binds tighter.
    if helpers::node_types::is_operator_method(name) && call.call_operator_loc().is_none() {
        return;
    }
    let parenthesized = call.closing_loc().is_some_and(|l| l.as_slice() == b")");
    let Some(last) = last_argument(call.arguments(), call.block()) else { return };
    if parenthesized {
        return;
    }
    let Some(sel) = call.message_loc() else { return };
    add_parens(sel.end_offset(), send_range(call), last, src, edits);
}

/// `correct_send` on a `super` with arguments: only `correctable_send?` applies.
fn correct_super(sup: &ruby_prism::SuperNode, src: &str, edits: &mut Vec<Edit>) {
    let Some(last) = last_argument(sup.arguments(), sup.block()) else { return };
    if sup.lparen_loc().is_some() {
        return;
    }
    // The parser `super` node excludes an attached `do`/`{}` block.
    let kw = sup.keyword_loc();
    let end = sup.block().filter(|b| b.as_block_node().is_some()).map_or(sup.location().end_offset(), |_| last.1);
    add_parens(kw.end_offset(), (kw.start_offset(), end), last, src, edits);
}

/// `correct_block_receiver`: a method chained onto a `do` block (`bar baz do ... end.qux`) still
/// needs parentheses around the arguments of the call that takes the block.
fn correct_block_receiver(call: &ruby_prism::CallNode, src: &str, edits: &mut Vec<Edit>) {
    let mut receiver = call.receiver();
    while let Some(c) = receiver.as_ref().and_then(|r| r.as_call_node()).filter(|c| !has_block(c) && !match_with_lvasgn(c)) {
        receiver = c.receiver();
    }
    let Some(r) = receiver else { return };
    if let Some(c) = r.as_call_node().filter(has_block) {
        correct_send(&c, &r, false, src, edits);
    } else if let Some(sup) = r.as_super_node().filter(|s| s.block().is_some_and(|b| b.as_block_node().is_some())) {
        correct_super(&sup, src, edits);
    }
}

/// `whitespace_before_arg` and the parentheses around the arguments.
fn add_parens(begin: usize, (s, e): (usize, usize), last: (usize, usize), src: &str, edits: &mut Vec<Edit>) {
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
        kind::DEFINED_NODE => node.as_defined_node().unwrap().lparen_loc(),
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
