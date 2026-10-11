use crate::helpers::comment_association::{self, Associator};
use crate::helpers::literal_node::parser_type;
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use helpers::select_by::plain_send;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Style/RequireOrder";
pub static COP: RequireOrder = RequireOrder;
pub struct RequireOrder;

/// The file's Comment::Associator result (node range -> comment indices), built on first use.
#[derive(Default)]
struct Associated(Option<Vec<(usize, usize, Vec<usize>)>>);

/// A modifier `if`/`unless`: Some(its single body statement); a non-modifier one: None.
/// Other nodes: Err(()).
fn modifier_body<'pr>(n: &Node<'pr>) -> Result<Option<Node<'pr>>, ()> {
    let (modifier, stmts) = match n {
        Node::IfNode { .. } => {
            let i = n.as_if_node().unwrap();
            (i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if") && i.end_keyword_loc().is_none(), i.statements())
        }
        Node::UnlessNode { .. } => {
            let u = n.as_unless_node().unwrap();
            (u.end_keyword_loc().is_none(), u.statements())
        }
        _ => return Err(()),
    };
    if !modifier {
        return Ok(None);
    }
    let mut it = stmts.into_iter().flat_map(|s| s.body().iter());
    let first = it.next();
    Ok(if it.next().is_some() { None } else { first })
}

fn arguments<'pr>(c: &CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut args = call_args(c);
    args.extend(c.block().filter(|b| b.as_block_argument_node().is_some()));
    args
}

fn str_value(n: Option<&Node>) -> Option<Vec<u8>> {
    let n = n?;
    if parser_type(n) != "str" {
        return None;
    }
    // `__FILE__` (a parser str) has no literal value here
    n.as_string_node().map(|s| s.unescaped().to_vec())
}

impl Cop for RequireOrder {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = plain_send(node) else { return };
        let method = method_name(&call);
        if call.receiver().is_some() || !matches!(method, "require" | "require_relative") {
            return;
        }
        let args = arguments(&call);
        if args.is_empty() {
            return;
        }
        let anc = cx.ancestors;
        let n = anc.len();
        // the search node (the require, or its modifier `if`) and the index of its statement list
        let (search, stmts_idx) = match anc.last() {
            Some(Node::StatementsNode { .. }) if anc[n - 1].as_statements_node().unwrap().body().iter().count() >= 2 => {
                ((node.location().start_offset(), node.location().end_offset()), n - 1)
            }
            Some(Node::StatementsNode { .. }) if n >= 2 && matches!(modifier_body(&anc[n - 2]), Ok(Some(_))) => {
                let l = anc[n - 2].location();
                ((l.start_offset(), l.end_offset()), n - 3)
            }
            Some(p @ (Node::IfNode { .. } | Node::UnlessNode { .. })) if modifier_body(p).is_ok_and(|b| b.is_some()) => {
                let l = p.location();
                ((l.start_offset(), l.end_offset()), n - 2)
            }
            // ponytail: other parents (arguments, array elements) are not searched for siblings
            _ => return,
        };
        let Some(stmts) = anc.get(stmts_idx).and_then(|s| s.as_statements_node()) else { return };
        let list: Vec<Node> = stmts.body().iter().collect();
        if list.len() < 2 {
            return;
        }
        let Some(pos) = list.iter().position(|s| s.location().start_offset() == search.0) else { return };
        let src = cx.src;
        let node_end = send_range(&call).1;
        let mine = str_value(args.first());
        let mut found = None;
        for k in (0..pos).rev() {
            let s = &list[k];
            let sibling = match modifier_body(s) {
                Err(()) => Some(s.as_call_node()),
                Ok(None) => None,
                // if_inside_only_require
                Ok(Some(body)) => plain_send(&body)
                    .filter(|c| {
                        c.receiver().is_none()
                            && matches!(method_name(c), "require" | "require_relative")
                            && arguments(c).len() == 1
                    })
                    .map(Some),
            };
            let Some(sib) = sibling.flatten().and_then(|c| plain_send(&c.as_node())) else { break };
            if method_name(&sib) != method || sib.receiver().is_some() || arguments(&sib).is_empty() {
                break;
            }
            if src.slice(sib.location().start_offset(), node_end).contains("\n\n") {
                break;
            }
            let (Some(a), Some(b)) = (&mine, str_value(arguments(&sib).first())) else { break };
            if *a < b {
                found = Some(k);
                break;
            }
        }
        let Some(k) = found else { return };
        // range_with_comments_and_lines: the statement joined with the comments parser's
        // Comment::Associator gives it (`ast_with_comments[node]`), whole lines. A comment on a
        // `def m # c` header line is a leading comment of the first body statement, since no
        // earlier child of the def has a location to take it (#326).
        let nodes = cx.state::<Associated>().0.get_or_insert_with(|| {
            let mut a = Associator::new(src, comment_association::skipped_directives(src));
            a.visit(&anc[0]);
            a.nodes
        });
        let with_comments = |i: usize| {
            let l = list[i].location();
            let (mut start, mut end) = (l.start_offset(), l.end_offset());
            let comments = nodes.iter().find(|n| n.0 == start && n.1 == end).into_iter().flat_map(|n| n.2.iter());
            for &c in comments {
                start = start.min(src.comments[c].start);
                end = end.max(src.comments[c].end);
            }
            range_by_whole_lines(src.text, start, end, true)
        };
        let range1 = with_comments(k);
        let range2 = with_comments(pos);
        let moved = src.slice(range2.0, range2.1).to_string();
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, format!("Sort `{method}` in alphabetical order."), |c| {
            c.remove(range2.0, range2.1);
            c.insert_before(range1.0, range1.1, &moved);
        });
    }
}
