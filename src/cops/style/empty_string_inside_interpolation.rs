use crate::helpers::if_node::{Body, Else};
use crate::prelude::*;

pub const NAME: &str = "Style/EmptyStringInsideInterpolation";
pub static COP: EmptyStringInsideInterpolation = EmptyStringInsideInterpolation;
pub struct EmptyStringInsideInterpolation;

const MSG_TRAILING_CONDITIONAL: &str = "Do not use trailing conditionals in string interpolation.";
const MSG_TERNARY: &str = "Do not return empty strings in string interpolation.";

/// `empty_branch_outcome?`: a `nil` or `''` branch.
fn empty_outcome(body: Option<&Body>) -> bool {
    body.and_then(|b| b.single()).is_some_and(|n| match &n {
        Node::NilNode { .. } => true,
        Node::StringNode { .. } => !is_parser_dstr(&n) && n.as_string_node().unwrap().unescaped().is_empty(),
        _ => false,
    })
}

/// The parser range of a branch body.
fn body_range(body: &Body) -> (usize, usize) {
    match body.single() {
        Some(n) => (n.location().start_offset(), n.location().end_offset()),
        None => body.range(),
    }
}

/// The parser range of an else branch: an `elsif` node or the else body.
fn else_range(els: &Option<Else>) -> Option<(usize, usize)> {
    match els {
        Some(Else::Elsif(n)) => Some((n.location().start_offset(), helpers::if_node::parser_end(n))),
        Some(Else::Body(b)) => b.as_ref().map(body_range),
        None => None,
    }
}

/// A conditional directly inside an interpolation (`node.parent.parent` is a dstr, dsym, regexp
/// or xstr). The visitor enters no StatementsNode under an EmbeddedStatementsNode (a concrete
/// child field), so the interpolation is the nearest entered ancestor.
fn in_interpolation(anc: &[Node]) -> bool {
    matches!(anc, [.., p] if p.as_embedded_statements_node().is_some())
}

/// Every if/unless below `root` (exclusive) with its ancestors from `root` down.
fn descendant_ifs<'pr>(root: &Node<'pr>, mut f: impl FnMut(&Node<'pr>, &[Node<'pr>]) -> bool) -> bool {
    struct V<'pr, 'f> {
        stack: Vec<Node<'pr>>,
        f: &'f mut dyn FnMut(&Node<'pr>, &[Node<'pr>]) -> bool,
        found: bool,
    }
    impl<'pr> ruby_prism::Visit<'pr> for V<'pr, '_> {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            if !self.found
                && !self.stack.is_empty()
                && matches!(kind::of(&node), kind::IF_NODE | kind::UNLESS_NODE)
                && (self.f)(&node, &self.stack)
            {
                self.found = true;
            }
            self.stack.push(node);
        }
        fn visit_branch_node_leave(&mut self) {
            self.stack.pop();
        }
    }
    let mut v = V { stack: Vec::new(), f: &mut f, found: false };
    ruby_prism::Visit::visit(&mut v, root);
    v.found
}

/// `command_call?(branch)`: an unparenthesized call, super or yield with arguments (not an
/// operator or setter); its `method(arguments)` form.
fn ternary_branch_source(src: &str, branch: &Node) -> String {
    let (start, end) = (branch.location().start_offset(), branch.location().end_offset());
    let command = match branch {
        Node::CallNode { .. } => {
            let c = branch.as_call_node().unwrap();
            let operator = helpers::node_types::is_operator_method(method_name(&c)) || c.is_attribute_write();
            let block = c.block().is_some_and(|b| b.as_block_node().is_some());
            (!operator && !block && c.opening_loc().is_none()).then(|| send_args(&c))
        }
        Node::SuperNode { .. } => {
            let s = branch.as_super_node().unwrap();
            let mut args: Vec<Node> = s.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default();
            args.extend(s.block().filter(|b| b.as_block_argument_node().is_some()));
            let block = s.block().is_some_and(|b| b.as_block_node().is_some());
            (s.lparen_loc().is_none() && !block).then_some(args)
        }
        Node::YieldNode { .. } => {
            let y = branch.as_yield_node().unwrap();
            y.lparen_loc().is_none().then(|| y.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default())
        }
        _ => None,
    };
    match command.filter(|a| !a.is_empty()) {
        Some(args) => {
            let (a0, a1) = (args[0].location().start_offset(), args.last().unwrap().location().end_offset());
            format!("{}({})", src[start..a0].trim_end(), &src[a0..a1])
        }
        None => src[start..end].to_string(),
    }
}

impl Cop for EmptyStringInsideInterpolation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::EMBEDDED_STATEMENTS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(stmts) = node.as_embedded_statements_node().unwrap().statements() else { return };
        let ternary_style = match cx.cfg.str("EnforcedStyle") {
            Some("trailing_conditional") => false,
            Some("ternary") => true,
            _ => return,
        };
        let src = cx.src.text;
        for child in stmts.body().iter() {
            let Some(cond) = helpers::if_node::cond(&child) else { continue };
            let (_, if_branch, els) = helpers::if_node::parts(&child);
            let cond_src = node_text(&cond.condition);
            let (s, e) = (child.location().start_offset(), child.location().end_offset());
            if ternary_style {
                if !cond.modifier() {
                    continue;
                }
                let (ns, ne) = (node.location().start_offset(), node.location().end_offset());
                // nested_trailing_conditional?: the rewrite would leave the inner one untouched.
                let nested = descendant_ifs(&child, |n, anc| {
                    helpers::if_node::cond(n).is_some_and(|c| c.modifier()) && in_interpolation(anc)
                });
                let branch = if_branch.as_ref().and_then(|b| b.single()).map_or_else(
                    || if_branch.as_ref().map_or(String::new(), |b| src[b.range().0..b.range().1].to_string()),
                    |n| ternary_branch_source(src, &n),
                );
                let outcomes = if cond.keyword == "unless" { format!("'' : {branch}") } else { format!("{branch} : ''") };
                let replacement = format!("{cond_src} ? {outcomes}");
                cx.add_fix(ns, ne, MSG_TRAILING_CONDITIONAL, |c| {
                    if !nested {
                        c.replace(s, e, &replacement);
                    }
                });
                continue;
            }
            if cond.modifier() {
                continue;
            }
            // empty_string_conditional_in_condition?: correcting both at once would clash.
            let blocked = descendant_ifs(&cond.condition, |n, anc| {
                let Some(c) = helpers::if_node::cond(n) else { return false };
                if c.modifier() || !in_interpolation(anc) {
                    return false;
                }
                let (_, ib, el) = helpers::if_node::parts(n);
                let eb = match &el {
                    Some(Else::Body(b)) => b.as_ref(),
                    _ => None,
                };
                empty_outcome(ib.as_ref()) || (!matches!(el, Some(Else::Elsif(_))) && empty_outcome(eb))
            });
            let else_branch = match &els {
                Some(Else::Body(b)) => b.as_ref(),
                _ => None,
            };
            // An `elsif` else branch is an if node, never an empty outcome.
            let else_empty = !matches!(els, Some(Else::Elsif(_))) && empty_outcome(else_branch);
            let correct = |cx: &mut Cx, outcome: Option<(usize, usize)>, keyword: &str| -> bool {
                let Some((os, oe)) = outcome else {
                    // `nil.source_range` raises: RuboCop drops the rest of this interpolation.
                    if !blocked {
                        return false;
                    }
                    cx.add_fix(s, e, MSG_TERNARY, |_| {});
                    return true;
                };
                let tail = format!(" {keyword} {cond_src}");
                cx.add_fix(s, e, MSG_TERNARY, |c| {
                    if !blocked {
                        c.remove(s, os);
                        c.replace(oe, e, &tail);
                    }
                });
                true
            };
            if empty_outcome(if_branch.as_ref()) && !correct(cx, else_range(&els), "unless") {
                return;
            }
            if else_empty && !correct(cx, if_branch.as_ref().map(body_range), "if") {
                return;
            }
        }
    }
}
