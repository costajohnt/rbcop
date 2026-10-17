use crate::prelude::*;
use helpers::variable_force::{self as vf, Tree};
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "RSpec/LeakyLocalVariable";
pub static COP: LeakyLocalVariable = LeakyLocalVariable;
pub struct LeakyLocalVariable;

type Span = (usize, usize);

/// The RSpec constructs the cop asks about, by range.
#[derive(Default)]
struct Spans {
    /// example_scope?: subject, let, hook, example or include (block or send form)
    scopes: Vec<(usize, usize)>,
    /// plain-block spec groups (inside_describe_block?)
    groups: Vec<(usize, usize)>,
    /// sends named like an example: (send range, argument ranges)
    examples: Vec<(Span, Vec<Span>)>,
    /// receiver-less include sends: (send range, structural keys of the arguments after the
    /// first that are not dstr/dsym, each with the keys of its descendants)
    includes: Vec<(Span, Vec<Vec<String>>)>,
}

fn inside(outer: (usize, usize), inner: (usize, usize)) -> bool {
    outer.0 <= inner.0 && inner.1 <= outer.1 && outer != inner
}

impl Cop for LeakyLocalVariable {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let src = cx.src;
        struct V<'a> {
            l: &'a helpers::rspec::Language,
            target: f64,
            s: Spans,
        }
        impl<'pr> Visit<'pr> for V<'_> {
            fn visit_call_node(&mut self, n: &CallNode<'pr>) {
                let node = n.as_node();
                let l = self.l;
                let range = helpers::rspec::node_range(&node);
                if helpers::rspec::subject(l, &node)
                    || helpers::rspec::let_(l, &node)
                    || helpers::rspec::hook(l, &node)
                    || helpers::rspec::example(l, &node)
                    || helpers::rspec::include_(l, &node)
                {
                    self.s.scopes.push(range);
                }
                if helpers::rspec::plain_block_call(&node).is_some() && helpers::rspec::spec_group(l, &node) {
                    self.s.groups.push(range);
                }
                // the send of a block call is a node of its own: the include send form
                if helpers::rspec::block_of(n).is_some_and(|b| !helpers::rspec::is_plain(&b))
                    && n.receiver().is_none()
                    && l.include(method_name(n))
                {
                    self.s.scopes.push(send_range(n));
                }
                if !is_csend(n) {
                    let args = send_args(n);
                    let name = method_name(n);
                    if l.example(name) {
                        self.s.examples.push((send_range(n), args.iter().map(helpers::rspec::node_range).collect()));
                    }
                    if n.receiver().is_none() && l.include(name) {
                        let keys = args
                            .iter()
                            .skip(1)
                            .filter(|a| {
                                !(helpers::rspec::is_dstr(a) || a.as_interpolated_symbol_node().is_some() || is_parser_dsym(a, self.target))
                            })
                            .map(|a| {
                                struct D(Vec<String>);
                                impl<'pr> Visit<'pr> for D {
                                    fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
                                        self.0.push(format!("lvar:{}", String::from_utf8_lossy(n.name().as_slice())));
                                    }
                                }
                                let mut d = D(Vec::new());
                                d.visit(a);
                                d.0
                            })
                            .collect();
                        self.s.includes.push((send_range(n), keys));
                    }
                }
                ruby_prism::visit_call_node(self, n);
            }
        }
        let mut v = V { l: &l, target: src.target_ruby, s: Spans::default() };
        v.visit(node);
        let spans = v.s;
        let tree = Tree::shared(&node.as_program_node().unwrap(), src);
        let mut found: Vec<(usize, usize)> = Vec::new();
        vf::investigate(tree, |tree, scope, vars| {
            for &vi in &scope.variables {
                let var = &vars[vi];
                let key = format!("lvar:{}", var.name);
                for a in &var.assignments {
                    let ar = tree.range(a.node);
                    if spans.scopes.iter().any(|&s| inside(s, ar)) {
                        continue;
                    }
                    for &r in &a.references {
                        let rr = tree.range(r);
                        if !spans.groups.iter().any(|&g| inside(g, rr)) || !spans.scopes.iter().any(|&s| inside(s, rr)) {
                            continue;
                        }
                        // allowed_reference?
                        let allowed = spans
                            .examples
                            .iter()
                            .any(|(s, args)| inside(*s, rr) && args.iter().any(|&a| a == rr || inside(a, rr)))
                            || spans.includes.iter().any(|(s, keys)| inside(*s, rr) && keys.iter().all(|k| !k.contains(&key)));
                        if !allowed {
                            found.push(ar);
                        }
                    }
                }
            }
        });
        for (s, e) in found {
            cx.add(s, e, "Do not use local variables defined outside of examples inside of them.");
        }
    }
}
