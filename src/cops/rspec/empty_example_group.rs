use crate::prelude::*;
use helpers::rspec::Language;
use ruby_prism::Visit;

pub const NAME: &str = "RSpec/EmptyExampleGroup";
pub static COP: EmptyExampleGroup = EmptyExampleGroup;
pub struct EmptyExampleGroup;

/// `{(block (send #rspec? {#Examples.all #ExampleGroups.all #Includes.all} ...) ...)
/// (send nil? {#Examples.all #Includes.all} ...)}`
fn example_or_group_or_include(l: &Language, node: &Node) -> bool {
    if let Some(c) = helpers::rspec::plain_block_call(node) {
        let n = method_name(&c);
        return helpers::rspec::rspec_recv(&c) && (l.example(n) || l.example_group(n) || l.include(n));
    }
    helpers::rspec::send_call(node).is_some_and(|c| {
        let n = method_name(&c);
        c.receiver().is_none() && (l.example(n) || l.include(n))
    })
}

/// `(block !(send nil? #Hooks.all ...) _ #examples?)`
fn examples_inside_block(l: &Language, node: &Node) -> bool {
    let Some(c) = helpers::rspec::plain_block_call(node) else { return false };
    // `!(send nil? #Hooks.all ...)`: a csend block call is not that send, so it counts (#426).
    if c.receiver().is_none() && l.hook(method_name(&c)) {
        return false;
    }
    helpers::rspec::parser_body(helpers::rspec::block_body(&c)).is_some_and(|b| examples(l, &b))
}

fn directly_or_in_block(l: &Language, node: &Node) -> bool {
    example_or_group_or_include(l, node) || examples_inside_block(l, node)
}

fn in_branches(l: &Language, node: &Node) -> bool {
    helpers::rspec::branches(node).is_some_and(|bs| bs.iter().flatten().any(|b| examples(l, b)))
}

/// `examples?`
fn examples(l: &Language, node: &Node) -> bool {
    directly_or_in_block(l, node)
        || in_branches(l, node)
        || helpers::rspec::begin_children(node)
            .is_some_and(|cs| cs.iter().any(|c| directly_or_in_block(l, c)) || cs.iter().any(|c| in_branches(l, c)))
}

/// conditionals_with_examples?: a `begin` or `case` body with an if/case descendant whose branches
/// hold examples.
fn conditionals_with_examples(l: &Language, body: &Node) -> bool {
    if helpers::rspec::begin_children(body).is_none() && body.as_case_node().is_none() {
        return false;
    }
    struct V<'a> {
        l: &'a Language,
        found: bool,
        root: (usize, usize),
    }
    impl<'pr> Visit<'pr> for V<'_> {
        fn visit_branch_node_enter(&mut self, node: Node<'pr>) {
            if self.found {
                return;
            }
            let is_cond = matches!(kind::of(&node), kind::IF_NODE | kind::UNLESS_NODE | kind::CASE_NODE);
            if is_cond && helpers::rspec::node_range(&node) != self.root && in_branches(self.l, &node) {
                self.found = true;
            }
        }
        fn visit_in_node(&mut self, node: &ruby_prism::InNode<'pr>) {
            // `in pat if guard`: parser's if_guard is no `if` node
            let p = node.pattern();
            if let Some(i) = p.as_if_node() {
                self.visit(&i.predicate());
                if let Some(s) = i.statements() {
                    self.visit_statements_node(&s);
                }
            } else if let Some(u) = p.as_unless_node() {
                self.visit(&u.predicate());
                if let Some(s) = u.statements() {
                    self.visit_statements_node(&s);
                }
            } else {
                self.visit(&p);
            }
            if let Some(s) = node.statements() {
                self.visit_statements_node(&s);
            }
        }
    }
    let mut v = V { l, found: false, root: helpers::rspec::node_range(body) };
    v.visit(body);
    v.found
}

impl Cop for EmptyExampleGroup {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        // example_group_body: (block (send #rspec? #ExampleGroups.all ...) args $_)
        let Some(call) = helpers::rspec::plain_block_call(node) else { return };
        if !helpers::rspec::rspec_recv(&call) || !l.example_group(method_name(&call)) {
            return;
        }
        if cx.ancestors.iter().any(|a| a.as_def_node().is_some()) {
            return;
        }
        // inside_example?: a plain-block ancestor that is an example
        if cx.ancestors.iter().any(|a| helpers::rspec::example(&l, a)) {
            return;
        }
        let body = helpers::rspec::parser_body(helpers::rspec::block_body(&call));
        let offensive = match &body {
            None => true,
            Some(b) if conditionals_with_examples(&l, b) => false,
            Some(b) if matches!(kind::of(b), kind::IF_NODE | kind::UNLESS_NODE | kind::CASE_NODE) => {
                !in_branches(&l, b)
            }
            Some(b) => !examples(&l, b),
        };
        if !offensive {
            return;
        }
        let (s, e) = send_range(&call);
        let (ns, ne) = helpers::rspec::node_range(node);
        let (rs, re) = helpers::whole_lines::range_by_whole_lines(cx.src.text, ns, ne, true);
        cx.add_fix(s, e, "Empty example group detected.", |c| c.remove(rs, re));
    }
}
