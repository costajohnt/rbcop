use crate::prelude::*;
use helpers::rspec::Language;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/RepeatedSubjectCall";
pub static COP: RepeatedSubjectCall = RepeatedSubjectCall;
pub struct RepeatedSubjectCall;

/// subject?: `(block (send nil? {#Subjects.all (sym $_) | $#Subjects.all}) args ...)`: its name.
fn subject_name(target: f64, l: &Language, node: &Node) -> Option<String> {
    let call = helpers::rspec::plain_block_call(node)?;
    if call.receiver().is_some() || !l.subject(method_name(&call)) {
        return None;
    }
    match &send_args(&call)[..] {
        [] => Some(method_name(&call).to_string()),
        [a] if a.as_symbol_node().is_some() && !is_parser_dsym(a, target) => helpers::rails::sym_value(a),
        _ => None,
    }
}

/// A parser parent as the subject check sees it.
#[derive(Clone, Copy)]
enum Frame {
    /// Prism-only wrapper
    Transparent,
    /// a `send` (true) or `csend`, with its receiver's range
    Call(bool, Option<(usize, usize)>),
    Other,
}

struct Walk<'a> {
    l: &'a Language,
    /// subjects_by_node: names defined in each example group (by its range)
    by_group: Vec<((usize, usize), String)>,
    /// plain blocks around the current node: (range, is an `expect` block)
    blocks: Vec<(usize, usize, bool)>,
    found: &'a mut Vec<(usize, usize)>,
}

impl Walk<'_> {
    /// detect_offenses_in_block over parser `send`, `def`, `block` and `begin` nodes.
    fn node(&mut self, node: &Node, names: &[String]) {
        let mut names = names.to_vec();
        let range = helpers::rspec::node_range(node);
        names.extend(self.by_group.iter().filter(|(r, _)| *r == range).map(|(_, n)| n.clone()));
        if helpers::rspec::example(self.l, node) {
            self.example(node, &names);
            return;
        }
        if let Some(call) = helpers::rspec::any_block_call(node) {
            let plain = helpers::rspec::is_plain(&helpers::rspec::block_of(&call).unwrap());
            self.blocks.push((range.0, range.1, plain && method_name(&call) == "expect"));
            if !is_csend(&call) {
                self.send_children(&call, &names);
            }
            if let Some(body) = helpers::rspec::parser_body(helpers::rspec::block_body(&call)) {
                self.child(&body, &names);
            }
            self.blocks.pop();
        } else if let Some(call) = node.as_call_node() {
            self.send_children(&call, &names);
        } else if let Some(def) = node.as_def_node() {
            if let Some(body) = helpers::rspec::parser_body(def.body()) {
                self.child(&body, &names);
            }
        } else if let Some(children) = helpers::rspec::begin_children(node) {
            for c in children {
                self.child(&c, &names);
            }
        }
    }

    fn send_children(&mut self, call: &CallNode, names: &[String]) {
        for c in call.receiver().into_iter().chain(send_args(call)) {
            self.child(&c, names);
        }
    }

    /// `each_child_node(:send, :def, :block, :begin)`
    fn child(&mut self, node: &Node, names: &[String]) {
        let ok = match node.as_call_node() {
            Some(c) => match helpers::rspec::block_of(&c) {
                Some(b) => helpers::rspec::is_plain(&b),
                None => !is_csend(&c),
            },
            None => {
                node.as_def_node().is_some_and(|d| d.receiver().is_none())
                    || helpers::rspec::begin_children(node).is_some()
            }
        };
        if ok {
            self.node(node, names);
        }
    }

    /// detect_offenses_in_example
    fn example(&mut self, node: &Node, names: &[String]) {
        let call = node.as_call_node().unwrap();
        let Some(body) = helpers::rspec::block_body(&call) else { return };
        struct V<'a> {
            names: &'a [String],
            blocks: Vec<(usize, usize, bool)>,
            frames: Vec<Frame>,
            used: Vec<String>,
            found: Vec<(usize, usize)>,
        }
        impl V<'_> {
            fn parent(&self) -> Frame {
                self.frames.iter().rev().copied().find(|f| !matches!(f, Frame::Transparent)).unwrap_or(Frame::Other)
            }
        }
        impl<'pr> Visit<'pr> for V<'_> {
            fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
                let f = match kind::of(&node) {
                    kind::CALL_NODE | kind::ARGUMENTS_NODE => Frame::Transparent,
                    kind::STATEMENTS_NODE if node.as_statements_node().unwrap().body().iter().count() < 2 => Frame::Transparent,
                    _ => Frame::Other,
                };
                self.frames.push(f);
            }
            fn visit_branch_node_leave(&mut self) {
                self.frames.pop();
            }
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                let name = method_name(n);
                let me = helpers::rspec::node_range(&n.as_node());
                // subject_calls: (send nil? %) for the subject names
                if n.receiver().is_none()
                    && send_args(n).is_empty()
                    && (name == "subject" || self.names.iter().any(|x| x == name))
                {
                    if self.used.iter().any(|u| u == name) {
                        // detect_offense: not chained, not a send's argument, inside an `expect` block
                        let ok = match self.parent() {
                            Frame::Call(send, recv) => !send && recv != Some(me),
                            _ => true,
                        };
                        if ok && let Some(&(s, e, _)) = self.blocks.iter().rev().find(|b| b.2) {
                            self.found.push((s, e));
                        }
                    } else {
                        self.used.push(name.to_string());
                    }
                }
                let recv = n.receiver().map(|r| helpers::rspec::node_range(&r));
                self.frames.push(Frame::Call(!is_csend(n), recv));
                if let Some(r) = n.receiver() {
                    self.visit(&r);
                }
                if let Some(a) = n.arguments() {
                    self.visit_arguments_node(&a);
                }
                if let Some(b) = n.block().filter(|b| b.as_block_node().is_none()) {
                    self.visit(&b);
                }
                self.frames.pop();
                if let Some(b) = helpers::rspec::block_of(n) {
                    let plain = helpers::rspec::is_plain(&b);
                    self.blocks.push((me.0, me.1, plain && name == "expect"));
                    self.frames.push(Frame::Other);
                    if let Some(params) = b.parameters() {
                        self.visit(&params);
                    }
                    if let Some(body) = b.body() {
                        self.visit(&body);
                    }
                    self.frames.pop();
                    self.blocks.pop();
                }
            }
        }
        let mut blocks = self.blocks.clone();
        let r = helpers::rspec::node_range(node);
        blocks.push((r.0, r.1, false));
        let mut v = V { names, blocks, frames: vec![Frame::Other], used: Vec::new(), found: Vec::new() };
        v.visit(&body);
        self.found.extend(v.found);
    }
}

impl Cop for RepeatedSubjectCall {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let target = cx.src.target_ruby;
        helpers::rspec::each_top_level_group(&l, node, |_, group, _| {
            // detect_subjects_in_scope: each subject block below the group, by its nearest example
            // group block (each_ancestor(:block).find, nearest first: only a subject's block
            // ancestors are asked, up to the first example group)
            struct S<'a, 'pr> {
                target: f64,
                l: &'a Language,
                blocks: Vec<Node<'pr>>,
                out: Vec<((usize, usize), String)>,
            }
            impl<'pr> Visit<'pr> for S<'_, 'pr> {
                fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                    let node = n.as_node();
                    if let Some(name) = subject_name(self.target, self.l, &node) {
                        let outer = self
                            .blocks
                            .iter()
                            .rev()
                            .find(|a| helpers::rspec::example_group(self.l, a))
                            .map_or((usize::MAX, usize::MAX), helpers::rspec::node_range);
                        self.out.push((outer, name));
                    }
                    let block = helpers::rspec::plain_block_call(&node).is_some();
                    if block {
                        self.blocks.push(node);
                    }
                    ruby_prism::visit_call_node(self, n);
                    if block {
                        self.blocks.pop();
                    }
                }
            }
            let mut s = S { target, l: &l, blocks: Vec::new(), out: Vec::new() };
            // each_descendant: the group itself is no subject of its own
            let c = group.as_call_node().unwrap();
            if helpers::rspec::plain_block_call(group).is_some() {
                s.blocks.push(c.as_node());
            }
            if let Some(r) = c.receiver() {
                s.visit(&r);
            }
            if let Some(a) = c.arguments() {
                s.visit_arguments_node(&a);
            }
            if let Some(b) = c.block() {
                s.visit(&b);
            }
            let by_group = s.out;
            // add_offense is immediate: the offenses found before a raise are kept
            helpers::rspec::add_found(
                cx,
                |_, found| Walk { l: &l, by_group, blocks: Vec::new(), found }.node(group, &[]),
                |cx, (s, e)| cx.add(s, e, "Calls to subject are memoized, this block is misleading"),
            );
        });
    }
}
