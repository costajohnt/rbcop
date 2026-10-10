use crate::prelude::*;
use helpers::parser_path::{self, Frame};
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Style/RedundantDoubleSplatHashBraces";
pub static COP: RedundantDoubleSplatHashBraces = RedundantDoubleSplatHashBraces;
pub struct RedundantDoubleSplatHashBraces;

const MSG: &str = "Remove the redundant double splat and braces, use keyword arguments directly.";

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

fn is_merge(name: &str) -> bool {
    matches!(name, "merge" | "merge!")
}

/// `mergeable?(parent)` up a parser ancestor chain (nearest first).
fn chain_mergeable(frames: &[Frame]) -> bool {
    for f in frames {
        if !f.is_call() {
            return true;
        }
        if !is_merge(f.name) {
            return false;
        }
    }
    true
}

fn has_block_node(call: &CallNode) -> bool {
    call.block().is_some_and(|b| b.as_block_node().is_some())
}

/// rubocop-ast `receiver` chain: `receiver.receiver ? root_receiver(receiver) : receiver`, where a
/// call with a block is a parser block (no receiver).
fn root_is_hash(call: &CallNode) -> bool {
    let mut recv = call.receiver();
    loop {
        let Some(r) = recv else { return false };
        let next = r.as_call_node().filter(|c| !has_block_node(c)).and_then(|c| c.receiver());
        if next.is_none() {
            return r.as_hash_node().is_some();
        }
        recv = next;
    }
}

struct Calls<'pr> {
    found: Vec<CallNode<'pr>>,
}

impl<'pr> Visit<'pr> for Calls<'pr> {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        self.found.push(node.as_node().as_call_node().unwrap());
        ruby_prism::visit_call_node(self, node);
    }
}

/// `hash_argument_source` / `"**#{arg.source}"`
fn argument_source(arg: &Node) -> String {
    if let Some(h) = arg.as_hash_node() {
        return h
            .elements()
            .iter()
            .filter(|e| e.as_assoc_node().is_some())
            .map(|e| node_text(&e).to_string())
            .collect::<Vec<_>>()
            .join(", ");
    }
    if arg.as_keyword_hash_node().is_some() {
        return node_text(arg).to_string();
    }
    format!("**{}", node_text(arg))
}

impl Cop for RedundantDoubleSplatHashBraces {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::HASH_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let hash = node.as_hash_node().unwrap();
        let elements: Vec<Node> = hash.elements().iter().collect();
        let pairs: Vec<_> = elements.iter().filter_map(|e| e.as_assoc_node()).collect();
        if pairs.is_empty() || pairs.iter().any(|p| p.operator_loc().is_some_and(|o| o.as_slice() == b"=>")) {
            return;
        }
        let anc = cx.ancestors;
        let chain: Vec<&Node> = anc.iter().collect();
        let mut frames = parser_path::frames(&chain, node);
        frames.reverse();
        let Some(parent) = frames.first() else { return };
        let Some(pi) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return };
        if !(parent.is_call() || anc[pi].as_assoc_splat_node().is_some()) {
            return;
        }
        if !chain_mergeable(&frames) {
            return;
        }
        let Some(ki) = anc.iter().rposition(|a| a.as_assoc_splat_node().is_some()) else { return };
        let kwsplat = anc[ki].as_assoc_splat_node().unwrap();
        // allowed_double_splat_receiver?
        let Some(first_child) = kwsplat.value() else { return };
        if first_child.as_lambda_node().is_some() {
            return;
        }
        if first_child.as_call_node().is_some_and(|c| has_block_node(&c) || !root_is_hash(&c)) {
            return;
        }
        let (Some(first), Some(last)) = (elements.first(), elements.last()) else { return };
        let (open, close) = (hash.opening_loc(), hash.closing_loc());
        let op = kwsplat.operator_loc();
        let open_r = (open.start_offset(), range(first).0);
        let close_r = (range(last).1, close.end_offset());

        // merge methods among kwsplat.each_descendant(:call), in pre-order
        let mut calls = Calls { found: Vec::new() };
        calls.visit(&kwsplat.as_node());
        let outer = &anc[..ki];
        let kw_node = kwsplat.as_node();
        let mergeable = |c: &CallNode| {
            if !is_merge(method_name(c)) {
                return false;
            }
            if has_block_node(c) {
                return true;
            }
            parser_path::ancestors(outer, &kw_node, &c.as_node()).is_some_and(|f| chain_mergeable(&f))
        };
        let flags: Vec<bool> = calls.found.iter().map(&mergeable).collect();
        let mut merge_fix: Option<((usize, usize), String)> = None;
        let merges: Vec<&CallNode> = calls.found.iter().zip(&flags).filter(|(_, f)| **f).map(|(c, _)| c).collect();
        if let (Some(outermost), Some(innermost)) = (merges.first(), merges.last()) {
            // RuboCop raises on `loc.dot` of a receiverless merge: no offense.
            let Some(dot) = innermost.call_operator_loc() else { return };
            let range = (dot.start_offset(), send_range(outermost).1);
            let mut groups: Vec<String> = calls
                .found
                .iter()
                .zip(&flags)
                .filter(|(_, f)| **f)
                .map(|(c, _)| send_args(c).iter().map(argument_source).collect::<Vec<_>>().join(", "))
                .collect();
            groups.reverse();
            groups.insert(0, String::new());
            merge_fix = Some((range, groups.join(", ")));
        }
        let (ks, ke) = range(&kwsplat.as_node());
        cx.add_fix(ks, ke, MSG, |c| {
            c.remove(op.start_offset(), op.end_offset());
            c.remove(open_r.0, open_r.1);
            c.remove(close_r.0, close_r.1);
            if let Some(((s, e), text)) = &merge_fix {
                c.replace(*s, *e, text);
            }
        });
    }
}
