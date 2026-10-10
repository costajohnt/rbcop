use crate::helpers::node_equality;
use crate::helpers::range_help::{Side, Surrounding, range_with_surrounding_space};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/AccessorGrouping";
pub static COP: AccessorGrouping = AccessorGrouping;
pub struct AccessorGrouping;

/// A parser `send` (not csend, not the send of a block).
fn as_send<'pr>(n: &Node<'pr>) -> Option<CallNode<'pr>> {
    n.as_call_node().filter(|c| !is_csend(c) && !c.block().is_some_and(|b| b.as_block_node().is_some()))
}

/// rubocop-ast `attribute_accessor?`.
fn attribute_accessor(call: &CallNode) -> bool {
    call.receiver().is_none()
        && matches!(method_name(call), "attr_reader" | "attr_writer" | "attr_accessor" | "attr")
        && !send_args(call).is_empty()
}

/// `(send nil? {:public :protected :private :module_function} ...)` (access_modifier?, bare or
/// not; a class-body sibling is always in macro scope).
fn access_modifier(call: &CallNode) -> bool {
    call.receiver().is_none() && matches!(method_name(call), "public" | "protected" | "private" | "module_function")
}

/// `visibility_block?`: `(send nil? {:private :protected :public})`
fn visibility_block<'pr>(n: &Node<'pr>) -> Option<&'pr str> {
    let call = as_send(n)?;
    let name = method_name(&call);
    (call.receiver().is_none() && send_args(&call).is_empty() && matches!(name, "private" | "protected" | "public"))
        .then_some(name)
}

struct Body<'a, 'pr> {
    src: &'a crate::source::Source<'a>,
    stmts: &'a [Node<'pr>],
}

impl<'pr> Body<'_, 'pr> {
    fn line(&self, off: usize) -> usize {
        self.src.line_of(off)
    }

    /// `previous_line_comment?`: `processed_source[first_line - 2]` (index -1 is the last line).
    fn previous_line_comment(&self, call: &CallNode) -> bool {
        let first = self.line(call.location().start_offset());
        let lines = &self.src.lines;
        let line = if first >= 2 { lines.get(first - 2) } else { lines.last() };
        line.is_some_and(|l| l.trim_start_matches([' ', '\t', '\n', '\r', '\x0c', '\x0b']).starts_with('#'))
    }

    /// `groupable_accessor?` for the statement at `i`.
    fn groupable_accessor(&self, i: usize, call: &CallNode) -> bool {
        let Some(prev) = i.checked_sub(1).map(|j| &self.stmts[j]) else { return true };
        let prev_send = match prev.as_call_node() {
            Some(c) if c.block().is_some_and(|b| b.as_block_node().is_some()) => {
                // a block: its first `send` child (the block's own send, or a single-send body)
                if is_csend(&c) {
                    block_body_send(&c.block().unwrap())
                } else {
                    Some(c)
                }
            }
            Some(_) => as_send(prev),
            None => prev.as_lambda_node().and_then(|l| l.body()).and_then(|b| single_send(&b)),
        };
        let Some(prev) = prev_send else { return true };
        let (ps, pe) = send_range(&prev);
        let prev_line = self.line(ps);
        if self.src.comments.iter().any(|c| self.line(c.start) == prev_line && self.src.slice(c.start, c.end).starts_with("#:")) {
            return false;
        }
        attribute_accessor(&prev)
            || access_modifier(&prev)
            || self.line(call.location().start_offset()) as i64 - self.line(pe) as i64 > 1
    }

    fn visibility(&self, i: usize) -> &'pr str {
        self.stmts[..i].iter().rev().find_map(visibility_block).unwrap_or("public")
    }

    /// `groupable_sibling_accessor?(node, sibling)`
    fn groupable_sibling(&self, i: usize, j: usize) -> bool {
        let (Some(node), Some(sib)) = (as_send(&self.stmts[i]), as_send(&self.stmts[j])) else { return false };
        attribute_accessor(&sib)
            && method_name(&sib) == method_name(&node)
            && self.visibility(j) == self.visibility(i)
            && self.groupable_accessor(j, &sib)
            && !self.previous_line_comment(&sib)
    }

    fn groupable_siblings(&self, i: usize) -> Vec<usize> {
        (0..self.stmts.len()).filter(|&j| self.groupable_sibling(i, j)).collect()
    }

    /// `skip_for_grouping?`
    fn skip_for_grouping(&self, i: usize) -> bool {
        let right = &self.stmts[i + 1..];
        right.iter().any(|n| n.as_constant_write_node().is_some() || n.as_constant_path_write_node().is_some())
            && (i + 1..self.stmts.len()).any(|j| as_send(&self.stmts[j]).is_some() && self.groupable_sibling(i, j))
    }
}

fn single_send<'pr>(body: &Node<'pr>) -> Option<CallNode<'pr>> {
    let stmts = body.as_statements_node()?;
    let mut it = stmts.body().iter();
    let (Some(first), None) = (it.next(), it.next()) else { return None };
    as_send(&first)
}

/// Parser children of a lone class-body statement that can hold sends: a block's (or lambda's)
/// single-send body (its other children are never accessors), an `if`'s condition and branches.
fn single_statement_children<'pr>(stmt: &Node<'pr>) -> Vec<Node<'pr>> {
    let one = |s: Option<ruby_prism::StatementsNode<'pr>>| -> Option<Node<'pr>> {
        let s = s?;
        let mut it = s.body().iter();
        match (it.next(), it.next()) {
            (Some(first), None) => Some(first),
            (Some(_), Some(_)) => Some(s.as_node()),
            _ => None,
        }
    };
    if let Some(c) = stmt.as_call_node() {
        return c.block().and_then(|b| b.as_block_node()).and_then(|b| b.body()).and_then(|b| one(b.as_statements_node())).into_iter().collect();
    }
    if let Some(l) = stmt.as_lambda_node() {
        return l.body().and_then(|b| one(b.as_statements_node())).into_iter().collect();
    }
    if let Some(i) = stmt.as_if_node().filter(|i| i.if_keyword_loc().is_none_or(|k| k.as_slice() == b"if")) {
        let mut out = vec![i.predicate()];
        out.extend(one(i.statements()));
        match i.subsequent() {
            Some(e) if e.as_else_node().is_some() => out.extend(one(e.as_else_node().unwrap().statements())),
            Some(e) => out.push(e),
            None => {}
        }
        return out;
    }
    Vec::new()
}

fn block_body_send<'pr>(block: &Node<'pr>) -> Option<CallNode<'pr>> {
    block.as_block_node()?.body().and_then(|b| single_send(&b))
}

impl Cop for AccessorGrouping {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::MODULE_NODE, kind::SINGLETON_CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let body = match node {
            Node::ClassNode { .. } => node.as_class_node().unwrap().body(),
            Node::ModuleNode { .. } => node.as_module_node().unwrap().body(),
            _ => node.as_singleton_class_node().unwrap().body(),
        };
        // class_send_elements: a single send, the sends of a multi-statement body, or the child
        // sends of a single other statement.
        // ponytail: only block/lambda and `if` children are modelled (not unless, rescue, ...).
        let Some(stmts) = body.and_then(|b| b.as_statements_node()) else { return };
        let mut stmts: Vec<Node> = stmts.body().iter().collect();
        if stmts.len() == 1 && as_send(&stmts[0]).is_none() {
            stmts = single_statement_children(&stmts[0]);
        }
        let grouped = cx.cfg.str("EnforcedStyle").unwrap_or("grouped") == "grouped";
        let b = Body { src: cx.src, stmts: &stmts };
        for (i, stmt) in stmts.iter().enumerate() {
            let Some(call) = as_send(stmt).filter(attribute_accessor) else { continue };
            if b.previous_line_comment(&call) || !b.groupable_accessor(i, &call) {
                continue;
            }
            let args = send_args(&call);
            if !(grouped && b.groupable_siblings(i).len() > 1 || !grouped && args.len() > 1) {
                continue;
            }
            let name = method_name(&call);
            let (s, e) = send_range(&call);
            let msg = if grouped {
                format!("Group together all `{name}` attributes.")
            } else {
                format!("Use one attribute per `{name}`.")
            };
            let src = cx.src;
            if grouped {
                let accessors = b.groupable_siblings(i);
                let preferred = (!b.skip_for_grouping(i) && (accessors[0] == i || b.skip_for_grouping(accessors[0])))
                    .then(|| {
                        let mut names: Vec<&str> = Vec::new();
                        for &j in &accessors {
                            for a in send_args(&as_send(&stmts[j]).unwrap()) {
                                let t = node_text(&a);
                                if !names.contains(&t) {
                                    names.push(t);
                                }
                            }
                        }
                        format!("{name} {}", names.join(", "))
                    });
                match preferred {
                    Some(text) => cx.add_fix(s, e, msg, |c| c.replace(s, e, &text)),
                    None => {
                        let opts = Surrounding { side: Side::Left, ..Default::default() };
                        let (rs, re) = range_with_surrounding_space(src.text, s, e, opts);
                        cx.add_fix(s, e, msg, |c| c.remove(rs, re));
                    }
                }
            } else {
                let comments = arg_comments(src, s, &args);
                let first = node_range(&args[0]);
                let indent = " ".repeat(src.column(s));
                let mut lines = Vec::new();
                for (a, cs) in args.iter().zip(&comments) {
                    let mut own: Vec<String> = cs.iter().map(|c| src.slice(c.start, c.end).to_string()).collect();
                    own.push(format!("{name} {}", node_text(a)));
                    if !node_equality::same(src, node_range(a), first) {
                        own = own.into_iter().map(|l| format!("{indent}{l}")).collect();
                    }
                    lines.extend(own);
                }
                let text = lines.join("\n");
                // range_with_trailing_argument_comment
                let end = comments.iter().filter_map(|cs| cs.last()).find(|c| c.start >= e).map_or(e, |c| c.end);
                cx.add_fix(s, e, msg, |c| c.replace(s, end, &text));
            }
        }
    }
}

fn node_range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

/// `processed_source.ast_with_comments[arg]` for each argument of a send starting at `start`:
/// parser's associator walks the arguments with one comment cursor, giving each the comments that
/// end before its end and those starting on its last line.
fn arg_comments<'a>(src: &'a crate::source::Source, start: usize, args: &[Node]) -> Vec<Vec<&'a Comment>> {
    let mut cur = src.comments.iter().position(|c| c.start >= start).unwrap_or(src.comments.len());
    args.iter()
        .map(|a| {
            let (_, end) = node_range(a);
            let mut out = Vec::new();
            while let Some(c) = src.comments.get(cur).filter(|c| c.end <= end) {
                out.push(c);
                cur += 1;
            }
            while let Some(c) = src.comments.get(cur).filter(|c| src.line_of(c.start) == src.line_of(end)) {
                out.push(c);
                cur += 1;
            }
            out
        })
        .collect()
}
