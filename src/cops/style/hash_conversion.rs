use crate::helpers::add_parentheses;
use crate::helpers::parser_parent::{is_operator_keyword, is_top_const, parser_parent_index};
use crate::prelude::*;

pub const NAME: &str = "Style/HashConversion";
pub static COP: HashConversion = HashConversion;
pub struct HashConversion;

const MSG_TO_H: &str = "Prefer `ary.to_h` to `Hash[ary]`.";
const MSG_LITERAL_MULTI_ARG: &str = "Prefer literal hash to `Hash[arg1, arg2, ...]`.";
const MSG_LITERAL_HASH_ARG: &str = "Prefer literal hash to `Hash[key: value, ...]`.";
const MSG_SPLAT: &str = "Prefer `array_of_pairs.to_h` to `Hash[*array]`.";

fn parenthesized(call: &ruby_prism::CallNode) -> bool {
    call.closing_loc().is_some_and(|l| l.as_slice() == b")")
}

/// rubocop-ast `call_type?` without an attached block (that is a parser `block`).
fn plain_call<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

fn requires_parens(node: &Node) -> bool {
    if let Some(c) = plain_call(node) {
        if method_name(&c) == "[]" {
            return false;
        }
        if !send_args(&c).is_empty() && !parenthesized(&c) {
            return true;
        }
    }
    is_operator_keyword(node)
}

/// add_parentheses_to_parent_call: walk up while the parent starts where the node does
/// (`Hash[a, b].merge(c)`), then the parent if it is an unparenthesized call, `super` or `yield`
/// that does not take its argument with operator syntax (`foo[x]`, `foo.bar = x`, `foo == x`).
fn parent_call<'a, 'pr>(ancestors: &'a [Node<'pr>], start: usize) -> Option<&'a Node<'pr>> {
    let mut i = ancestors.len();
    let parent = loop {
        let p = parser_parent_index(ancestors, i)?;
        if ancestors[p].location().start_offset() != start {
            break &ancestors[p];
        }
        i = p;
    };
    match parent {
        Node::CallNode { .. } => {
            let c = parent.as_call_node().unwrap();
            let operator_syntax = c.equal_loc().is_some()
                || (helpers::node_types::is_operator_method(method_name(&c)) && c.call_operator_loc().is_none());
            (!parenthesized(&c) && !operator_syntax).then_some(parent)
        }
        Node::SuperNode { .. } => {
            parent.as_super_node().unwrap().rparen_loc().is_none().then_some(parent)
        }
        Node::YieldNode { .. } => {
            parent.as_yield_node().unwrap().rparen_loc().is_none().then_some(parent)
        }
        _ => None,
    }
}

impl Cop for HashConversion {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "[]" || is_csend(&call) {
            return;
        }
        let (start, end) = send_range(&call);
        if cx.state::<Vec<(usize, usize)>>().iter().any(|&(s, e)| s <= start && end <= e) {
            return;
        }
        if !call.receiver().is_some_and(|r| is_top_const(&r, "Hash")) {
            return;
        }
        cx.state::<Vec<(usize, usize)>>().push((start, end));
        let args = send_args(&call);
        let src = cx.src.text;
        // add_parentheses_to_parent_call: the add_parentheses fix of the call the hash literal
        // would start the first argument of, if it needs one.
        let parent = parent_call(cx.ancestors, start).map(|p| add_parentheses::plan(p, src));
        if let [first] = args.as_slice() {
            match first {
                Node::HashNode { .. } | Node::KeywordHashNode { .. } => {
                    let replacement = format!("{{{}}}", node_text(first));
                    let paren = parent;
                    // add_parentheses raises where the plan is None (no selector).
                    if matches!(paren, Some(None)) {
                        return;
                    }
                    cx.add_fix(start, end, MSG_LITERAL_HASH_ARG, |c| {
                        c.replace(start, end, &replacement);
                        if let Some(Some(fix)) = &paren {
                            add_parentheses::apply(c, fix);
                        }
                    });
                }
                Node::SplatNode { .. } => {
                    if !cx.cfg.bool("AllowSplatArgument").unwrap_or(true) {
                        cx.add_with(start, end, MSG_SPLAT, false);
                    }
                }
                _ => {
                    if let Some(zip) = plain_call(first).filter(|z| {
                        !is_csend(z) && method_name(z) == "zip" && send_args(z).is_empty()
                    }) {
                        let insert = if parenthesized(&zip) {
                            let l = zip.closing_loc().unwrap();
                            (l.start_offset(), l.end_offset(), true)
                        } else {
                            let (s, e) = send_range(&zip);
                            (s, e, false)
                        };
                        cx.add_fix(start, end, MSG_TO_H, |c| {
                            if insert.2 {
                                c.insert_before(insert.0, insert.1, "[]");
                            } else {
                                c.insert_after(insert.0, insert.1, "([])");
                            }
                        });
                    } else {
                        let mut replacement = node_text(first).to_string();
                        if requires_parens(first) {
                            replacement = format!("({replacement})");
                        }
                        let replacement = format!("{replacement}.to_h");
                        cx.add_fix(start, end, MSG_TO_H, |c| c.replace(start, end, &replacement));
                    }
                }
            }
            return;
        }
        if args.iter().any(|a| a.as_splat_node().is_some()) {
            return;
        }
        if args.len() % 2 == 1 {
            cx.add_with(start, end, MSG_LITERAL_MULTI_ARG, false);
            return;
        }
        let pairs: Vec<String> =
            args.chunks(2).map(|p| format!("{} => {}", node_text(&p[0]), node_text(&p[1]))).collect();
        let replacement = format!("{{{}}}", pairs.join(", "));
        let paren = parent;
        if matches!(paren, Some(None)) {
            return;
        }
        cx.add_fix(start, end, MSG_LITERAL_MULTI_ARG, |c| {
            c.replace(start, end, &replacement);
            if let Some(Some(fix)) = &paren {
                add_parentheses::apply(c, fix);
            }
        });
    }
}
