use crate::prelude::*;
use helpers::minitest::*;

pub const NAME: &str = "Minitest/EmptyLineBeforeAssertionMethods";
pub static COP: EmptyLineBeforeAssertionMethods = EmptyLineBeforeAssertionMethods;
pub struct EmptyLineBeforeAssertionMethods;

impl Cop for EmptyLineBeforeAssertionMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let anc = cx.ancestors;
        let block = call.block().and_then(|b| b.as_block_node());
        // assertion_method(node): the send itself, or the block it starts whose body is one.
        if as_send(node).is_some() {
            if !send_is_assertion(&call) {
                return;
            }
            // return if node.parent.resbody_type?
            if matches!(helpers::parser_parent::parser_parent(anc), Some(Node::RescueNode { .. })) {
                return;
            }
        } else {
            let Some(b) = &block else { return };
            // The send of a block is its first child: an assertion send has no left sibling.
            if send_is_assertion(&call) || block_call(node).is_none() || method_name(&call) == "test" {
                return;
            }
            if !parser_body(b.body()).is_some_and(|body| assertion_method(Some(&body))) {
                return;
            }
        }
        let target = range(node);
        let Some(prev) = left_sibling(anc, node) else { return };
        let prev = match prev {
            Sibling::Accept => return,
            Sibling::Node(n) => n,
        };
        if assertion_method(Some(&prev)) {
            return;
        }
        // use_heredoc_argument?: a call whose last argument is a heredoc stands for that heredoc.
        let prev = match as_call(&prev).and_then(|c| send_args(&c).into_iter().last()) {
            Some(last) if is_heredoc(&last) => last,
            _ => prev,
        };
        // use_assertion_method_at_last_of_block?
        if let Some(b) = block_call(&prev).and_then(|c| c.block()).and_then(|b| b.as_block_node())
            && let Some(body) = parser_body(b.body())
        {
            let last = match body.as_statements_node() {
                Some(s) => s.body().iter().last(),
                None => Some(body),
            };
            if assertion_method(last.as_ref()) {
                return;
            }
        }
        let heredoc_end = helpers::heredoc::parts(&prev).map(|(_, end, _)| end);
        let prev_line = match heredoc_end {
            Some(end) => cx.src.line_of(end.0),
            None => cx.src.line_of(range(&prev).1),
        };
        if prev_line + 1 != cx.src.line_of(target.0) {
            return;
        }
        let at = match heredoc_end {
            Some(end) => end,
            None => {
                let r = range(&prev);
                helpers::whole_lines::range_by_whole_lines(cx.src.text, r.0, r.1, true)
            }
        };
        let (s, e) = if block.is_some() { target } else { send_range(&call) };
        cx.add_fix(s, e, "Add empty line before assertion.", |c| c.insert_after(at.0, at.1, "\n"));
    }
}

enum Sibling<'pr> {
    /// Not a node (a method name), an `args` node, or the assertion's parent is a conditional.
    Accept,
    Node(Node<'pr>),
}

/// `assertion_method.left_sibling` with accept_previous_line?'s non-assertion checks.
fn left_sibling<'pr>(anc: &[Node<'pr>], node: &Node<'pr>) -> Option<Sibling<'pr>> {
    let me = range(node);
    let parent = anc.last()?;
    match parent {
        Node::StatementsNode { .. } => {
            let stmts: Vec<Node> = parent.as_statements_node().unwrap().body().iter().collect();
            if stmts.len() > 1 {
                let i = stmts.iter().position(|s| range(s) == me)?;
                return if i == 0 { None } else { Some(Sibling::Node(stmts.into_iter().nth(i - 1).unwrap())) };
            }
            // A single statement: the sibling comes from the construct owning the statements.
            let owner = anc.get(anc.len().checked_sub(2)?)?;
            match owner {
                // def/block/lambda bodies follow their `args`; conditionals accept.
                Node::DefNode { .. }
                | Node::BlockNode { .. }
                | Node::LambdaNode { .. }
                | Node::IfNode { .. }
                | Node::UnlessNode { .. }
                | Node::WhileNode { .. }
                | Node::UntilNode { .. } => Some(Sibling::Accept),
                Node::ElseNode { .. } => {
                    let grand = anc.get(anc.len().checked_sub(3)?)?;
                    matches!(grand, Node::IfNode { .. } | Node::UnlessNode { .. }).then_some(Sibling::Accept)
                }
                Node::WhenNode { .. } => {
                    owner.as_when_node().unwrap().conditions().iter().last().map(Sibling::Node)
                }
                Node::ForNode { .. } => Some(Sibling::Node(owner.as_for_node().unwrap().collection())),
                Node::ClassNode { .. } => owner.as_class_node().unwrap().superclass().map(Sibling::Node),
                Node::ModuleNode { .. } => Some(Sibling::Node(owner.as_module_node().unwrap().constant_path())),
                _ => None,
            }
        }
        Node::AndNode { .. } => {
            let n = parent.as_and_node().unwrap();
            (range(&n.right()) == me).then(|| Sibling::Node(n.left()))
        }
        Node::OrNode { .. } => {
            let n = parent.as_or_node().unwrap();
            (range(&n.right()) == me).then(|| Sibling::Node(n.left()))
        }
        Node::IfNode { .. } | Node::UnlessNode { .. } | Node::WhileNode { .. } | Node::UntilNode { .. } => {
            Some(Sibling::Accept)
        }
        Node::ArgumentsNode { .. } => {
            let args: Vec<Node> = parent.as_arguments_node().unwrap().arguments().iter().collect();
            let i = args.iter().position(|a| range(a) == me)?;
            if i == 0 {
                // A call's method name (a symbol child) precedes its first argument.
                return anc.get(anc.len().checked_sub(2)?)?.as_call_node().map(|_| Sibling::Accept);
            }
            Some(Sibling::Node(args.into_iter().nth(i - 1).unwrap()))
        }
        _ => None,
    }
}
