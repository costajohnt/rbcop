use crate::helpers::parser_parent::parser_parent_index;
use crate::helpers::regexp_parser::{self, ExprKind};
use crate::helpers::value_used::value_used;
use crate::prelude::*;

pub const NAME: &str = "Style/ExactRegexpMatch";
pub static COP: ExactRegexpMatch = ExactRegexpMatch;
pub struct ExactRegexpMatch;

/// `truthiness_only?(node)`: the value of the node starting at `start` (Prism ancestors `anc`)
/// is unused or only tested for truthiness.
fn truthiness_only(anc: &[Node], start: usize) -> bool {
    if !value_used(anc, start) {
        return true;
    }
    let Some(pi) = parser_parent_index(anc, anc.len()) else { return true };
    let parent = &anc[pi];
    let pstart = parent.location().start_offset();
    match parent {
        // parser `begin`
        Node::ParenthesesNode { .. } | Node::EmbeddedStatementsNode { .. } => truthiness_only(&anc[..pi], pstart),
        Node::StatementsNode { .. } => match pi.checked_sub(1).map(|w| &anc[w]) {
            // `begin ... end` is a kwbegin, which holds its statements directly.
            Some(Node::BeginNode { .. }) if anc[pi - 1].as_begin_node().unwrap().begin_keyword_loc().is_some() => false,
            Some(w @ (Node::ParenthesesNode { .. } | Node::EmbeddedStatementsNode { .. })) => {
                truthiness_only(&anc[..pi - 1], w.location().start_offset())
            }
            _ => truthiness_only(&anc[..pi], pstart),
        },
        Node::AndNode { .. } | Node::OrNode { .. } => truthiness_only(&anc[..pi], pstart),
        Node::IfNode { .. } => parent.as_if_node().unwrap().predicate().location().start_offset() == start,
        Node::UnlessNode { .. } => parent.as_unless_node().unwrap().predicate().location().start_offset() == start,
        Node::WhileNode { .. } => {
            let w = parent.as_while_node().unwrap();
            !w.is_begin_modifier() && w.predicate().location().start_offset() == start
        }
        Node::UntilNode { .. } => {
            let u = parent.as_until_node().unwrap();
            !u.is_begin_modifier() && u.predicate().location().start_offset() == start
        }
        Node::CallNode { .. } => {
            let c = parent.as_call_node().unwrap();
            method_name(&c) == "!" && !is_csend(&c)
        }
        _ => false,
    }
}

impl Cop for ExactRegexpMatch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "=~" | "===" | "!~" | "match" | "match?") {
            return;
        }
        let Some(receiver) = call.receiver() else { return };
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        // `String#===` is plain equality: `===` only matches with the regexp as its receiver.
        if name == "===" && is_csend(&call) {
            return;
        }
        let (regexp, subject): (&Node, &Node) = if name == "===" { (&receiver, arg) } else { (arg, &receiver) };
        // `(regexp (str $_) (regopt))`: one string part, no options.
        let Some(re) = regexp.as_regular_expression_node() else { return };
        let content = text(&re.content_loc());
        if content.is_empty() || content.contains('\n') || re.closing_loc().as_slice().len() != 1 {
            return;
        }
        let Some(tree) = regexp_parser::tree(content, false) else { return };
        let token = |i: usize, s: &str| tree.get(i).is_some_and(|e| &content[e.start..e.end] == s);
        let Some(lit) = tree.get(1) else { return };
        if !token(0, "\\A") || lit.kind != ExprKind::Literal || !token(2, "\\z") || lit.quantifier.is_some() {
            return;
        }
        let mut string = String::new();
        for ch in content[lit.start..lit.end].chars() {
            if ch == '\'' || ch == '\\' {
                string.push('\\');
            }
            string.push(ch);
        }
        let op = if name == "!~" { "!=" } else { "==" };
        let prefer = format!("{} {op} '{string}'", node_text(subject));
        let (start, end) = send_range(&call);
        // `=~` returns an index, `match` MatchData and `&.` maybe nil: only a condition may change.
        let boolean = !is_csend(&call) && matches!(name, "===" | "!~" | "match?");
        let fix = (boolean || truthiness_only(cx.ancestors, start)).then(|| {
            // parenthesize?
            let parent = helpers::parser_parent::parser_parent(cx.ancestors);
            let parenthesize = parent.and_then(|p| p.as_call_node()).is_some_and(|p| {
                let receiver = p.receiver().is_some_and(|r| r.location().start_offset() == start);
                let in_args = send_args(&p).iter().any(|a| a.location().start_offset() == start);
                receiver
                    || (in_args
                        && helpers::node_types::is_operator_method(method_name(&p))
                        && method_name(&p) != "[]")
            });
            if parenthesize { format!("({prefer})") } else { prefer.clone() }
        });
        cx.add_fix(start, end, format!("Use `{prefer}`."), |c| {
            if let Some(f) = &fix {
                c.replace(start, end, f);
            }
        });
    }
}
