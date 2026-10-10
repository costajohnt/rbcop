use crate::helpers::check_single_line_suitability::{suitable_as_single_line_skipping, to_single_line};
use crate::helpers::reparsed_equivalence::verified_by_reparse_folding;
use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Layout/RedundantLineBreak";
pub static COP: RedundantLineBreak = RedundantLineBreak;
pub struct RedundantLineBreak;

const MSG: &str = "Redundant line break detected.";

/// Ranges passed to `ignore_node`.
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

/// `/re/ =~ x` is parser `match_with_lvasgn`, not a send.
fn match_with_lvasgn(call: &ruby_prism::CallNode) -> bool {
    call.name().as_slice() == b"=~" && call.receiver().is_some_and(|r| r.as_regular_expression_node().is_some())
}

/// `{`/`do` and `}`/`end` of a block-owning node (call, super, lambda literal).
fn block_delims(n: &Node) -> Option<(usize, usize)> {
    let block = match n {
        Node::LambdaNode { .. } => {
            let l = n.as_lambda_node().unwrap();
            return Some((l.opening_loc().start_offset(), l.closing_loc().start_offset()));
        }
        Node::CallNode { .. } => n.as_call_node().unwrap().block(),
        Node::SuperNode { .. } => n.as_super_node().unwrap().block(),
        Node::ForwardingSuperNode { .. } => n.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        _ => None,
    }?;
    let b = block.as_block_node()?;
    Some((b.opening_loc().start_offset(), b.closing_loc().start_offset()))
}

/// Descendant blocks: (one is single-line with a dotted call as parent, one is multiline).
struct Blocks<'s> {
    src: &'s crate::source::Source<'s>,
    root: usize,
    skip_block: Option<usize>,
    /// Per entered node: it is a call with a dot (`.`, `&.`, `::`).
    stack: Vec<bool>,
    chained_single: bool,
    multiline: bool,
}

impl<'pr> Visit<'pr> for Blocks<'_> {
    fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
        let is_root = self.stack.is_empty() && node.location().start_offset() == self.root;
        if !is_root && let Some((open, close)) = block_delims(&node) {
            let single = self.src.line_of(open) == self.src.line_of(close);
            self.chained_single |= single && self.stack.last() == Some(&true);
            self.multiline |= !single;
        }
        self.stack.push(node.as_call_node().is_some_and(|c| c.call_operator_loc().is_some()));
    }
    fn visit_branch_node_leave(&mut self) {
        self.stack.pop();
    }
    fn visit_block_node(&mut self, node: &ruby_prism::BlockNode<'pr>) {
        if self.skip_block != Some(node.location().start_offset()) {
            ruby_prism::visit_block_node(self, node);
        }
    }
}

/// The parser node RuboCop checks: a Prism node plus whether it stands for the block (a call
/// with a block at block level, or a lambda literal).
struct Target<'a, 'pr> {
    node: &'a Node<'pr>,
    block_level: bool,
    /// The parser `send` inside an op-assignment (`a[i] ||= x`, `a.b += x`): its range and
    /// children (receiver, index arguments).
    inner: Option<((usize, usize), Vec<Node<'pr>>)>,
}

/// The `send` parser has inside an index/attribute op-assignment.
fn op_assign_send<'pr>(node: &Node<'pr>) -> Option<((usize, usize), Vec<Node<'pr>>)> {
    macro_rules! index {
        ($n:expr) => {{
            let n = $n;
            let recv = n.receiver()?;
            let mut parts = vec![recv];
            parts.extend(n.arguments().map(|a| a.arguments().iter().collect::<Vec<_>>()).unwrap_or_default());
            ((parts[0].location().start_offset(), n.closing_loc().end_offset()), parts)
        }};
    }
    macro_rules! attr {
        ($n:expr) => {{
            let n = $n;
            let recv = n.receiver()?;
            let end = n.message_loc()?.end_offset();
            ((recv.location().start_offset(), end), vec![recv])
        }};
    }
    Some(match node {
        Node::IndexOperatorWriteNode { .. } => index!(node.as_index_operator_write_node().unwrap()),
        Node::IndexOrWriteNode { .. } => index!(node.as_index_or_write_node().unwrap()),
        Node::IndexAndWriteNode { .. } => index!(node.as_index_and_write_node().unwrap()),
        Node::CallOperatorWriteNode { .. } => attr!(node.as_call_operator_write_node().unwrap()),
        Node::CallOrWriteNode { .. } => attr!(node.as_call_or_write_node().unwrap()),
        Node::CallAndWriteNode { .. } => attr!(node.as_call_and_write_node().unwrap()),
        _ => return None,
    })
}

impl Cop for RedundantLineBreak {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        static KINDS: LazyLock<Vec<usize>> = LazyLock::new(|| {
            let mut k = helpers::check_assignment::KINDS.to_vec();
            k.push(kind::LAMBDA_NODE);
            k
        });
        &KINDS
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::CallNode { .. } => {
                if match_with_lvasgn(&node.as_call_node().unwrap()) {
                    return;
                }
                on_send(cx, node, false);
            }
            // The `lambda` send of `-> {}` always climbs to its block.
            Node::LambdaNode { .. } => on_send(cx, node, true),
            _ => {
                if node.as_local_variable_write_node().is_some() && cx.src.text.ends_with("%\n\n") {
                    return;
                }
                let t = Target { node, block_level: false, inner: None };
                if offense(cx, &t) {
                    register(cx, &t);
                }
                // on_send for the op-assignment's inner send (`&.` ones are csends: also checked).
                if let Some(inner) = op_assign_send(node) {
                    let t = Target { node, block_level: false, inner: Some(inner) };
                    let (s, e) = target_range(&t);
                    if offense(cx, &t) && !cx.state::<Ignored>().0.iter().any(|&(is, ie)| is <= s && ie >= e) {
                        register(cx, &t);
                    }
                }
            }
        }
    }
}

fn on_send(cx: &mut Cx, node: &Node, block_level: bool) {
    let mut chain: Vec<&Node> = cx.ancestors.iter().collect();
    chain.push(node);
    let (mut j, mut block_level) = (chain.len() - 1, block_level);
    loop {
        let cur = chain[j];
        if !block_level
            && let Some(call) = cur.as_call_node()
            && call.block().is_some_and(|b| b.as_block_node().is_some())
        {
            // convertible_block?: the parent is the block of this send.
            let parenthesized = call.closing_loc().is_some_and(|c| c.as_slice() == b")");
            if parenthesized || send_args(&call).is_empty() {
                block_level = true;
                continue;
            }
            break;
        }
        // The parser parent, skipping Prism-only wrappers.
        let mut p = j;
        let parent = loop {
            if p == 0 {
                break None;
            }
            p -= 1;
            match chain[p] {
                Node::ArgumentsNode { .. } => continue,
                Node::StatementsNode { .. } if chain[p].as_statements_node().unwrap().body().iter().count() == 1 => {
                    continue;
                }
                _ => break Some(p),
            }
        };
        let Some(p) = parent else { break };
        match chain[p] {
            Node::CallNode { .. } => {
                let c = chain[p].as_call_node().unwrap();
                let direct = matches!(chain[p + 1], Node::ArgumentsNode { .. })
                    || c.receiver().is_some_and(|r| r.location().start_offset() == cur.location().start_offset() && p + 1 == j);
                if !direct || is_csend(&c) || match_with_lvasgn(&c) {
                    break;
                }
                j = p;
                block_level = false;
            }
            Node::AndNode { .. } | Node::OrNode { .. } => {
                j = p;
                block_level = false;
            }
            _ => break,
        }
    }
    let t = Target { node: chain[j], block_level, inner: None };
    if !offense(cx, &t) {
        return;
    }
    let (s, e) = target_range(&t);
    if cx.state::<Ignored>().0.iter().any(|&(is, ie)| is <= s && ie >= e) {
        return;
    }
    register(cx, &t);
}

/// The parser source range: a send owning a block stops before it.
fn target_range(t: &Target) -> (usize, usize) {
    if let Some((r, _)) = &t.inner {
        return *r;
    }
    match t.node.as_call_node() {
        Some(c) if !t.block_level => send_range(&c),
        _ => {
            let l = t.node.location();
            (l.start_offset(), l.end_offset())
        }
    }
}

/// A send whose Prism node owns a block: that block is not part of it.
fn own_block(t: &Target) -> Option<usize> {
    if t.block_level {
        return None;
    }
    let b = t.node.as_call_node()?.block()?;
    b.as_block_node().map(|b| b.location().start_offset())
}

fn offense(cx: &mut Cx, t: &Target) -> bool {
    let src = cx.src;
    let (s, e) = target_range(t);
    if let Some((_, parts)) = &t.inner {
        let index = matches!(
            t.node,
            Node::IndexOperatorWriteNode { .. } | Node::IndexOrWriteNode { .. } | Node::IndexAndWriteNode { .. }
        );
        if src.line_of(s) == src.line_of(e)
            || !parts.iter().all(|p| suitable_as_single_line_skipping(cx, p, s, e, None))
            || (index && is_index_send(&parts[0]))
        {
            return false;
        }
        let mut v = Blocks { src, root: usize::MAX, skip_block: None, stack: vec![], chained_single: false, multiline: false };
        for p in parts {
            v.visit(p);
        }
        return not_excluded(cx, &v, false);
    }
    let multiline = match block_delims(t.node).filter(|_| t.block_level || t.node.as_lambda_node().is_some()) {
        Some((open, close)) => src.line_of(open) != src.line_of(close),
        None => src.line_of(s) != src.line_of(e),
    };
    if !multiline || !suitable_as_single_line_skipping(cx, t.node, s, e, own_block(t)) {
        return false;
    }
    let op = match t.node {
        Node::AndNode { .. } => Some(t.node.as_and_node().unwrap().operator_loc()),
        Node::OrNode { .. } => Some(t.node.as_or_node().unwrap().operator_loc()),
        _ => None,
    };
    if let Some(op) = op {
        // require_backslash?
        return src.lines.get(src.line_of(op.start_offset()) - 1).is_some_and(|l| l.ends_with('\\'));
    }
    // index_access_call_chained?
    if !t.block_level
        && let Some(c) = t.node.as_call_node()
        && !is_csend(&c)
        && c.name().as_slice() == b"[]"
        && c.receiver().is_some_and(|r| is_index_send(&r))
    {
        return false;
    }
    let mut v = Blocks {
        src,
        root: t.node.location().start_offset(),
        skip_block: own_block(t),
        stack: vec![],
        chained_single: false,
        multiline: false,
    };
    v.visit(t.node);
    not_excluded(cx, &v, t.block_level || t.node.as_lambda_node().is_some())
}

/// A parser `send :[]` (not csend, not owning a block).
fn is_index_send(n: &Node) -> bool {
    n.as_call_node().is_some_and(|r| {
        !is_csend(&r) && r.name().as_slice() == b"[]" && !r.block().is_some_and(|b| b.as_block_node().is_some())
    })
}

/// `!configured_to_not_be_inspected?` given the node's descendant blocks.
fn not_excluded(cx: &Cx, v: &Blocks, is_block: bool) -> bool {
    if helpers::alignment::cop_enabled(cx, "Layout/SingleLineBlockChain") && v.chained_single {
        return false;
    }
    if cx.cfg.bool("InspectBlocks").unwrap_or(false) {
        return true;
    }
    !(is_block || v.multiline)
}

fn register(cx: &mut Cx, t: &Target) {
    let (s, e) = target_range(t);
    let single = to_single_line(cx.src.slice(s, e));
    let single = single.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\0').to_string();
    let Some(program) = cx.ancestors.first() else { return };
    let ok = verified_by_reparse_folding(
        cx.src.text,
        cx.src.version,
        program,
        &[(s, e)],
        |r| r,
        |c, r| c.replace(r.0, r.1, &single),
        true,
    );
    if ok.is_empty() {
        return;
    }
    cx.add_fix(s, e, MSG, |c| c.replace(s, e, &single));
    cx.state::<Ignored>().0.push((s, e));
}
