use crate::prelude::*;
use helpers::parser_parent::parser_parent;

pub const NAME: &str = "Performance/RedundantMatch";
pub static COP: RedundantMatch = RedundantMatch;
pub struct RedundantMatch;

const HIGHER_PRECEDENCE_OPERATOR_METHODS: &[&str] =
    &["|", "^", "&", "+", "-", "*", "/", "%", "**", ">", ">=", "<", "<=", "<<", ">>"];

fn str_lit(node: &Node) -> bool {
    matches!(kind::of(node), kind::STRING_NODE | kind::SOURCE_FILE_NODE) && !is_parser_dstr(node)
}

fn regexp(node: &Node) -> bool {
    matches!(kind::of(node), kind::REGULAR_EXPRESSION_NODE | kind::INTERPOLATED_REGULAR_EXPRESSION_NODE)
}

impl Cop for RedundantMatch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "match" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        // {(send {str regexp} :match _) (send !nil? :match {str regexp})}
        if !(str_lit(&recv) || regexp(&recv) || str_lit(arg) || regexp(arg)) {
            return;
        }
        // !(node.parent && node.parent.block_type?): its own block, or one whose body it is.
        if let Some(b) = call.block().and_then(|b| b.as_block_node()) {
            if plain(&b) {
                return;
            }
        } else if parser_parent(cx.ancestors).and_then(|p| p.as_block_node()).is_some_and(|b| plain(&b)) {
            return;
        }
        let start = node.location().start_offset();
        // The send of its own (numbered-parameter) block is value-used, outside any condition.
        let own_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        if own_block || (helpers::value_used::value_used(cx.ancestors, start) && !only_truthiness_matters(cx.ancestors, node)) {
            return;
        }
        let correctable = regexp(&recv) || regexp(arg);
        let replacement = if requires_parentheses(arg) { format!("({})", node_text(arg)) } else { node_text(arg).to_string() };
        let new_source = format!("{} =~ {replacement}", node_text(&recv));
        let (s, e) = send_range(&call);
        cx.add_fix(s, e, "Use `=~` in places where the `MatchData` returned by `#match` will not be used.", |c| {
            if correctable {
                c.replace(s, e, &new_source);
            }
        });
    }
}

fn plain(b: &ruby_prism::BlockNode) -> bool {
    !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}

/// `^({if while until case while_post until_post} equal?(%0) ...)`: the node is the condition.
fn only_truthiness_matters(anc: &[Node], node: &Node) -> bool {
    let Some(parent) = parser_parent(anc) else { return false };
    let start = node.location().start_offset();
    let end = node.location().end_offset();
    let is = |n: Option<Node>| n.is_some_and(|n| n.location().start_offset() == start && n.location().end_offset() == end);
    match parent {
        Node::IfNode { .. } => is(Some(parent.as_if_node().unwrap().predicate())),
        Node::UnlessNode { .. } => is(Some(parent.as_unless_node().unwrap().predicate())),
        Node::WhileNode { .. } => is(Some(parent.as_while_node().unwrap().predicate())),
        Node::UntilNode { .. } => is(Some(parent.as_until_node().unwrap().predicate())),
        Node::CaseNode { .. } => is(parent.as_case_node().unwrap().predicate()),
        _ => false,
    }
}

/// RedundantMatch#requires_parentheses?
fn requires_parentheses(arg: &Node) -> bool {
    if helpers::parser_parent::is_ternary(arg) || helpers::parser_parent::is_operator_keyword(arg) {
        return true;
    }
    if arg.as_range_node().is_some() {
        return true;
    }
    // call_like?(arg) && requires_parentheses_for_call_like?(arg)
    let (parenthesized, has_args, name) = match arg {
        Node::CallNode { .. } => {
            let c = arg.as_call_node().unwrap();
            if c.block().is_some_and(|b| b.as_block_node().is_some()) {
                return false;
            }
            (c.opening_loc().is_some_and(|o| o.as_slice() == b"("), !send_args(&c).is_empty(), method_name(&c))
        }
        Node::YieldNode { .. } => {
            let y = arg.as_yield_node().unwrap();
            (y.lparen_loc().is_some(), y.arguments().is_some(), "yield")
        }
        Node::SuperNode { .. } => {
            let s = arg.as_super_node().unwrap();
            if s.block().is_some_and(|b| b.as_block_node().is_some()) {
                return false;
            }
            (s.lparen_loc().is_some(), s.arguments().is_some() || s.block().is_some(), "super")
        }
        _ => return false,
    };
    !parenthesized && has_args && !HIGHER_PRECEDENCE_OPERATOR_METHODS.contains(&name)
}
