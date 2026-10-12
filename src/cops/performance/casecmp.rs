use crate::prelude::*;

pub const NAME: &str = "Performance/Casecmp";
pub static COP: Casecmp = Casecmp;
pub struct Casecmp;

/// A parser `send` (not csend, no literal block) named downcase/upcase; `bare` also wants no args.
fn case_send<'pr>(node: &Node<'pr>, bare: bool) -> Option<ruby_prism::CallNode<'pr>> {
    let c = node.as_call_node()?;
    if is_csend(&c) || !matches!(method_name(&c), "downcase" | "upcase") || c.block().is_some_and(|b| b.as_block_node().is_some()) {
        return None;
    }
    (!bare || send_args(&c).is_empty()).then_some(c)
}

fn str_lit(node: &Node) -> bool {
    matches!(kind::of(node), kind::STRING_NODE | kind::SOURCE_FILE_NODE) && !is_parser_dstr(node)
}

/// `{str (send _ {:downcase :upcase} ...) (begin str)}`
fn other_side(node: &Node) -> bool {
    str_lit(node)
        || case_send(node, false).is_some()
        || node.as_parentheses_node().and_then(|p| p.body()).and_then(|b| b.as_statements_node()).is_some_and(|s| {
            let body: Vec<Node> = s.body().iter().collect();
            matches!(body.as_slice(), [only] if str_lit(only))
        })
}

/// Util#parentheses?: `loc.end` is `)`.
fn parenthesized(node: &Node) -> bool {
    match node {
        Node::ParenthesesNode { .. } => true,
        Node::CallNode { .. } => node.as_call_node().unwrap().closing_loc().is_some_and(|c| c.as_slice() == b")"),
        Node::SuperNode { .. } => node.as_super_node().unwrap().rparen_loc().is_some(),
        Node::YieldNode { .. } => node.as_yield_node().unwrap().rparen_loc().is_some(),
        Node::DefinedNode { .. } => node.as_defined_node().unwrap().rparen_loc().is_some(),
        _ => false,
    }
}

impl Cop for Casecmp {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        if is_csend(&call) || !matches!(method, "==" | "eql?" | "!=") {
            return;
        }
        let Some(lhs) = call.receiver() else { return };
        let args = send_args(&call);
        let [rhs] = args.as_slice() else { return };
        let downcase_eq = case_send(&lhs, true).is_some() && other_side(rhs);
        let eq_downcase = other_side(&lhs) && case_send(rhs, true).is_some();
        if !downcase_eq && !eq_downcase {
            return;
        }
        // take_method_apart
        let (receiver, arg) = if case_send(&lhs, true).is_some() && case_send(rhs, true).is_some() {
            (lhs.as_call_node().unwrap(), rhs.as_call_node().unwrap().receiver().map(|a| info(&a)))
        } else if downcase_eq {
            (lhs.as_call_node().unwrap(), Some(info(rhs)))
        } else {
            (rhs.as_call_node().unwrap(), Some(info(&lhs)))
        };
        // `arg.send_type?` on nil, `variable.source` on nil: RuboCop raises, no offense.
        let (Some((arg, arg_send, arg_parens)), Some(variable)) = (arg, receiver.receiver()) else { return };
        let bang = if method == "!=" { "!" } else { "" };
        let good = if arg_send || !arg_parens {
            format!("{bang}{}.casecmp({arg}).zero?", node_text(&variable))
        } else {
            format!("{bang}{}.casecmp{arg}.zero?", node_text(&variable))
        };
        let (start, end) = send_range(&call);
        let msg = format!("Use `{good}` instead of `{}`.", cx.src.slice(start, end));
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &good));
    }
}

/// (source, send_type?, parentheses?) of the compared argument.
fn info<'pr>(node: &Node<'pr>) -> (&'pr str, bool, bool) {
    let send = node.as_call_node().is_some_and(|c| !is_csend(&c) && !c.block().is_some_and(|b| b.as_block_node().is_some()));
    (node_text(node), send, parenthesized(node))
}
