use crate::prelude::*;
use helpers::variable_force::{self as vf, T, Tree};
use ruby_prism::{CallNode, Visit};
use std::collections::HashMap;

pub const NAME: &str = "RSpec/LeakyLocalVariable";
pub static COP: LeakyLocalVariable = LeakyLocalVariable;
pub struct LeakyLocalVariable;

type Span = (usize, usize);

fn inside(outer: Span, inner: Span) -> bool {
    outer.0 <= inner.0 && inner.1 <= outer.1 && outer != inner
}

/// The calls of the file by the parser node they are: a block-form call by its whole range, a send
/// (of a block or not) by its send range.
#[derive(Default)]
struct Calls<'pr> {
    blocks: HashMap<Span, Node<'pr>>,
    sends: HashMap<Span, Node<'pr>>,
}

impl<'pr> Visit<'pr> for Calls<'pr> {
    fn visit_call_node(&mut self, n: &CallNode<'pr>) {
        if helpers::rspec::block_of(n).is_some() {
            self.blocks.insert(helpers::rspec::node_range(&n.as_node()), n.as_node());
        }
        if !is_csend(n) {
            self.sends.insert(send_range(n), n.as_node());
        }
        ruby_prism::visit_call_node(self, n);
    }
}

impl Cop for LeakyLocalVariable {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    // The Language predicates are asked lazily, in RuboCop's order: a list set to `~` raises
    // only when it is asked (#529).
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let l = helpers::rspec::lang(cx);
        let src = cx.src;
        let tree = Tree::shared(&node.as_program_node().unwrap(), src);
        let mut calls: Option<Calls> = None;
        let mut found: Vec<Span> = Vec::new();
        vf::investigate(tree, |tree, scope, vars| {
            let calls = calls.get_or_insert_with(|| {
                let mut c = Calls::default();
                c.visit(node);
                c
            });
            let block = |id: usize| tree.t(id).is_any_block().then(|| calls.blocks.get(&tree.range(id))).flatten().and_then(|n| n.as_call_node());
            let send = |id: usize| (tree.t(id) == T::Send).then(|| calls.sends.get(&tree.range(id))).flatten().and_then(|n| n.as_call_node());
            // example_scope?: subject? || let? || hook? || example? || include?
            let example_scope = |id: usize| {
                if let Some(c) = block(id) {
                    let plain = helpers::rspec::block_of(&c).is_some_and(|b| helpers::rspec::is_plain(&b));
                    let n = method_name(&c);
                    return c.receiver().is_none()
                        && ((plain && l.subject(n))
                            || (plain && l.helper(n))
                            || l.hook(n)
                            || (plain && l.example(n))
                            || (plain && l.include(n)));
                }
                let Some(c) = send(id) else { return false };
                let args = send_args(&c);
                let n = method_name(&c);
                // let?: (send nil? #Helpers.all _ block_pass); include?: (send nil? #Includes.all ...)
                let let_send = args.len() == 2 && kind::of(&args[1]) == kind::BLOCK_ARGUMENT_NODE;
                c.receiver().is_none() && ((let_send && l.helper(n)) || l.include(n))
            };
            let part_of_example_scope = |id: usize| tree.ancestors(id).any(example_scope);
            for &vi in &scope.variables {
                let var = &vars[vi];
                let key = format!("lvar:{}", var.name);
                for a in &var.assignments {
                    if part_of_example_scope(a.node) {
                        continue;
                    }
                    for &r in &a.references {
                        let rr = tree.range(r);
                        // inside_describe_block?: a plain-block ancestor that is a spec group
                        let in_group = tree.ancestors(r).filter(|&id| tree.t(id) == T::Block).any(|id| {
                            block(id).is_some_and(|c| {
                                let n = method_name(&c);
                                helpers::rspec::rspec_recv(&c) && (l.shared_group(n) || l.example_group(n))
                            })
                        });
                        if !in_group || !part_of_example_scope(r) {
                            continue;
                        }
                        // allowed_reference?
                        let allowed = tree.ancestors(r).any(|id| {
                            let Some(c) = send(id) else { return false };
                            let args = send_args(&c);
                            let n = method_name(&c);
                            // example_method?: (send nil? #Examples.all _)
                            if args.len() == 1 && c.receiver().is_none() && l.example(n) {
                                return true;
                            }
                            // in_example_arguments?
                            if l.example(n)
                                && args.iter().map(helpers::rspec::node_range).any(|ar| ar == rr || inside(ar, rr))
                            {
                                return true;
                            }
                            // includes_method? then allowed_includes_arguments?
                            if c.receiver().is_none() && l.include(n) {
                                return args.iter().skip(1).all(|a| {
                                    if helpers::rspec::is_dstr(a) || is_parser_dsym(a, src.target_ruby) {
                                        return true;
                                    }
                                    struct D<'k>(&'k str, bool);
                                    impl<'pr> Visit<'pr> for D<'_> {
                                        fn visit_local_variable_read_node(
                                            &mut self,
                                            n: &ruby_prism::LocalVariableReadNode<'pr>,
                                        ) {
                                            self.1 |= format!("lvar:{}", String::from_utf8_lossy(n.name().as_slice())) == self.0;
                                        }
                                    }
                                    let mut d = D(&key, false);
                                    d.visit(a);
                                    !d.1
                                });
                            }
                            false
                        });
                        if !allowed {
                            found.push(tree.range(a.node));
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
