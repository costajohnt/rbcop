use crate::helpers::node_equality::same;
use crate::helpers::parser_parent::{parser_parent, parser_parent_index};
use crate::prelude::*;
use crate::source::Source;

pub const NAME: &str = "Style/FetchEnvVar";
pub static COP: FetchEnvVar = FetchEnvVar;
pub struct FetchEnvVar;

type Range = (usize, usize);

fn range(node: &Node) -> Range {
    match node.as_call_node() {
        Some(c) if c.block().is_some_and(|b| b.as_block_node().is_some()) => {
            let l = node.location();
            (l.start_offset(), l.end_offset())
        }
        Some(c) => send_range(&c),
        None => {
            let l = node.location();
            (l.start_offset(), l.end_offset())
        }
    }
}

/// Parser `child_nodes` of a condition (the cases that can hold an `ENV[]` send).
fn child_nodes(node: &Node) -> Vec<Range> {
    match node {
        Node::CallNode { .. } => {
            let c = node.as_call_node().unwrap();
            let mut out: Vec<Range> = c.receiver().iter().map(range).collect();
            out.extend(send_args(&c).iter().map(range));
            out
        }
        Node::AndNode { .. } => {
            let a = node.as_and_node().unwrap();
            vec![range(&a.left()), range(&a.right())]
        }
        Node::OrNode { .. } => {
            let o = node.as_or_node().unwrap();
            vec![range(&o.left()), range(&o.right())]
        }
        Node::ParenthesesNode { .. } => match node.as_parentheses_node().unwrap().body() {
            Some(b) => match b.as_statements_node() {
                Some(s) => s.body().iter().map(|n| range(&n)).collect(),
                None => vec![range(&b)],
            },
            None => Vec::new(),
        },
        Node::DefinedNode { .. } => vec![range(&node.as_defined_node().unwrap().value())],
        _ => assignment_parts(node).map(|(_, v)| vec![range(&v)]).unwrap_or_default(),
    }
}

/// `condition.send_type?`: a send without `&.` and without an attached block.
fn is_send(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| !is_csend(&c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

fn eq_list(src: &Source, a: &[Range], b: &[Range]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| same(src, x, y))
}

const COMPARISON: &[&str] = &["==", "===", "!=", "<=", ">=", ">", "<"];

fn used_in_condition(src: &Source, node: Range, children: &[Range], condition: &Node) -> bool {
    if is_send(condition) {
        let c = condition.as_call_node().unwrap();
        let name = method_name(&c);
        let cond_children = child_nodes(condition);
        if !COMPARISON.contains(&name) && name.ends_with('=') {
            // partial_matched?: node.child_nodes == node.child_nodes & condition.child_nodes
            let mut inter: Vec<Range> = Vec::new();
            for &x in children {
                if cond_children.iter().any(|&y| same(src, x, y)) && !inter.iter().any(|&y| same(src, x, y)) {
                    inter.push(x);
                }
            }
            if eq_list(src, children, &inter) {
                return true;
            }
        }
        if !COMPARISON.contains(&name) && !name.ends_with('?') {
            return false;
        }
    }
    child_nodes(condition).iter().any(|&x| same(src, x, node))
}

impl Cop for FetchEnvVar {
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
        if !call.receiver().is_some_and(|r| r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"ENV")) {
            return;
        }
        let args = send_args(&call);
        let [name_node] = args.as_slice() else { return };
        let src = cx.src;
        let (start, end) = send_range(&call);
        let this = (start, end);
        let children = child_nodes(node);

        // allowed_var?
        if let Some(s) = name_node.as_string_node()
            && !is_parser_dstr(name_node)
            && cx.cfg.strings("AllowedVariables").iter().any(|v| v.as_bytes() == s.unescaped())
        {
            return;
        }
        let has_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        // The parser parent of the send (the block when it has one).
        let parent_i = if has_block { None } else { parser_parent_index(cx.ancestors, cx.ancestors.len()) };
        let parent = parent_i.map(|i| &cx.ancestors[i]);
        let root = !has_block && parent.is_none();

        // used_as_flag?
        if !root {
            let if_node = cx.ancestors.iter().enumerate().rev().find_map(|(i, a)| {
                let guard = i > 0 && is_pattern_guard(a, &cx.ancestors[..i]);
                let cond = match a {
                    Node::IfNode { .. } => Some(a.as_if_node().unwrap().predicate()),
                    Node::UnlessNode { .. } => Some(a.as_unless_node().unwrap().predicate()),
                    _ => None,
                };
                cond.filter(|_| !guard)
            });
            if let Some(condition) = if_node {
                if is_send(&condition) && eq_list(src, &child_nodes(&condition), &children) {
                    return;
                }
                if used_in_condition(src, this, &children, &condition) {
                    return;
                }
            }
            if let Some(p) = parent.filter(|p| is_send(p)) {
                let c = p.as_call_node().unwrap();
                let name = method_name(&c);
                let prefix_bang = name == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"!");
                if prefix_bang || COMPARISON.contains(&name) {
                    return;
                }
            }
        }
        if let Some(p) = parent {
            // message_chained_with_dot?
            if let Some(c) = p.as_call_node()
                && c.receiver().is_some_and(|r| same(src, range(&r), this))
                && c.call_operator_loc().is_some_and(|o| matches!(o.as_slice(), b"." | b"&."))
            {
                return;
            }
            // assigned?: a setter send whose receiver is this node.
            if let Some(c) = p.as_call_node()
                && c.equal_loc().is_some()
                && c.receiver().is_some_and(|r| same(src, range(&r), this))
            {
                return;
            }
            // or_lhs?
            if let Some(o) = p.as_or_node() {
                let grand_or = parent_i
                    .and_then(|i| parser_parent(&cx.ancestors[..i]))
                    .is_some_and(|g| g.as_or_node().is_some());
                if same(src, range(&o.left()), this) || grand_or {
                    return;
                }
            }
        }

        let key = node_text(name_node);
        let default_to_nil = cx.cfg.bool("DefaultToNil").unwrap_or(true);
        let (msg, replacement) = if default_to_nil {
            (format!("Use `ENV.fetch({key}, nil)` instead of `ENV[{key}]`."), format!("ENV.fetch({key}, nil)"))
        } else {
            (format!("Use `ENV.fetch({key})` instead of `ENV[{key}]`."), format!("ENV.fetch({key})"))
        };
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}
