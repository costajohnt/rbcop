use crate::prelude::*;
use helpers::rspec::Language;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/SubjectStub";
pub static COP: SubjectStub = SubjectStub;
pub struct SubjectStub;

/// message_expectation_matcher?: `(send nil? {:receive :receive_messages :receive_message_chain :have_received} ...)` in the node.
fn has_matcher(node: &Node) -> bool {
    struct V(bool);
    impl<'pr> Visit<'pr> for V {
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            self.0 |= n.receiver().is_none()
                && matches!(method_name(n), "receive" | "receive_messages" | "receive_message_chain" | "have_received");
            ruby_prism::visit_call_node(self, n);
        }
    }
    let mut v = V(false);
    v.visit(node);
    v.0
}

/// message_expectation?: `(send {(send nil? {:expect :allow} (send nil? %)) (send nil? :is_expected)} #Runners.all #message_expectation_matcher?)`
fn stub(node: &Node, names: &[String]) -> bool {
    let Some(call) = helpers::rspec::send_call(node).filter(|c| !is_csend(c)) else { return false };
    if !helpers::rspec::is_runner(method_name(&call)) {
        return false;
    }
    let [arg] = &send_args(&call)[..] else { return false };
    let Some(recv) = call.receiver().and_then(|r| helpers::rspec::send_call(&r)) else { return false };
    if recv.receiver().is_some() || is_csend(&recv) {
        return false;
    }
    let ok = match method_name(&recv) {
        "is_expected" => send_args(&recv).is_empty(),
        "expect" | "allow" => match &send_args(&recv)[..] {
            [s] => helpers::rspec::send_call(s).is_some_and(|s| {
                s.receiver().is_none() && send_args(&s).is_empty() && {
                    let n = method_name(&s);
                    n == "subject" || names.iter().any(|x| x == n)
                }
            }),
            _ => false,
        },
        _ => false,
    };
    ok && has_matcher(arg)
}

/// subject? / let?: `(block (send nil? {#Subjects.all (sym $_) | $#Subjects.all}) args ...)` and
/// `(block (send nil? :let (sym $_)) args ...)`.
fn definition(target: f64, l: &Language, node: &Node, lets: bool) -> Option<String> {
    let call = helpers::rspec::plain_block_call(node)?;
    let name = method_name(&call);
    if call.receiver().is_some() {
        return None;
    }
    let args = send_args(&call);
    let sym = |a: &Node| (a.as_symbol_node().is_some() && !is_parser_dsym(a, target)).then(|| helpers::rails::sym_value(a)).flatten();
    if lets {
        return match &args[..] {
            [a] if name == "let" => sym(a),
            _ => None,
        };
    }
    if !l.subject(name) {
        return None;
    }
    match &args[..] {
        [] => Some(name.to_string()),
        [a] => sym(a),
        _ => None,
    }
}

struct Walk<'a> {
    l: &'a Language,
    subjects: Vec<((usize, usize), String)>,
    overrides: Vec<((usize, usize), String)>,
    found: Vec<(usize, usize)>,
}

impl Walk<'_> {
    /// find_subject_expectations over parser `send`, `def`, `block` and `begin` nodes.
    fn node(&mut self, node: &Node, names: &[String]) {
        let range = helpers::rspec::node_range(node);
        let mut names: Vec<String> = names.to_vec();
        names.extend(self.subjects.iter().filter(|(r, _)| *r == range).map(|(_, n)| n.clone()));
        let overrides: Vec<&String> = self.overrides.iter().filter(|(r, _)| *r == range).map(|(_, n)| n).collect();
        names.retain(|n| !overrides.contains(&n));
        if stub(node, &names) {
            self.found.push(send_range(&node.as_call_node().unwrap()));
            return;
        }
        if let Some(call) = helpers::rspec::any_block_call(node) {
            if !is_csend(&call) {
                // the block's send is its first child
                if stub_send(&call, &names) {
                    self.found.push(send_range(&call));
                } else {
                    self.send_children(&call, &names);
                }
            }
            if let Some(body) = helpers::rspec::parser_body(helpers::rspec::block_body(&call)) {
                self.child(&body, &names);
            }
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
        let _ = self.l;
    }

    fn send_children(&mut self, call: &CallNode, names: &[String]) {
        for c in call.receiver().into_iter().chain(send_args(call)) {
            self.child(&c, names);
        }
    }

    fn child(&mut self, node: &Node, names: &[String]) {
        let ok = match node.as_call_node() {
            Some(c) => match helpers::rspec::block_of(&c) {
                Some(b) => helpers::rspec::is_plain(&b),
                None => !is_csend(&c),
            },
            None => node.as_def_node().is_some_and(|d| d.receiver().is_none()) || helpers::rspec::begin_children(node).is_some(),
        };
        if ok {
            self.node(node, names);
        }
    }
}

/// `stub` for the send part of a call with a block (`allow(subject).to receive(:x) do ... end`).
fn stub_send(call: &CallNode, names: &[String]) -> bool {
    if !helpers::rspec::is_runner(method_name(call)) {
        return false;
    }
    let [arg] = &send_args(call)[..] else { return false };
    let Some(recv) = call.receiver() else { return false };
    let Some(r) = helpers::rspec::send_call(&recv).filter(|r| r.receiver().is_none() && !is_csend(r)) else { return false };
    let ok = match method_name(&r) {
        "is_expected" => send_args(&r).is_empty(),
        "expect" | "allow" => matches!(&send_args(&r)[..], [s] if helpers::rspec::send_call(s).is_some_and(|s| {
            s.receiver().is_none() && send_args(&s).is_empty() && (method_name(&s) == "subject" || names.iter().any(|x| x == method_name(&s)))
        })),
        _ => false,
    };
    ok && has_matcher(arg)
}

impl Cop for SubjectStub {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let target = cx.src.target_ruby;
        let mut found = Vec::new();
        for group in helpers::rspec::top_level_groups(&l, node) {
            // find_all_explicit: definitions below the group, by their nearest example group block
            struct S<'a> {
                target: f64,
                l: &'a Language,
                groups: Vec<(usize, usize)>,
                subjects: Vec<((usize, usize), String)>,
                overrides: Vec<((usize, usize), String)>,
            }
            impl<'pr> Visit<'pr> for S<'_> {
                fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                    let node = n.as_node();
                    let outer = self.groups.last().copied().unwrap_or((usize::MAX, usize::MAX));
                    if let Some(name) = definition(self.target, self.l, &node, false) {
                        self.subjects.push((outer, name));
                    }
                    if let Some(name) = definition(self.target, self.l, &node, true) {
                        self.overrides.push((outer, name));
                    }
                    let group = helpers::rspec::plain_block_call(&node).is_some() && helpers::rspec::example_group(self.l, &node);
                    if group {
                        self.groups.push(helpers::rspec::node_range(&node));
                    }
                    ruby_prism::visit_call_node(self, n);
                    if group {
                        self.groups.pop();
                    }
                }
            }
            let mut s = S { target, l: &l, groups: Vec::new(), subjects: Vec::new(), overrides: Vec::new() };
            let c = group.as_call_node().unwrap();
            if helpers::rspec::plain_block_call(&group).is_some() && helpers::rspec::example_group(&l, &group) {
                s.groups.push(helpers::rspec::node_range(&group));
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
            let mut w = Walk { l: &l, subjects: s.subjects, overrides: s.overrides, found: Vec::new() };
            w.node(&group, &[]);
            found.extend(w.found);
        }
        for (s, e) in found {
            cx.add(s, e, "Do not stub methods of the object under test.");
        }
    }
}
