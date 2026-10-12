use crate::prelude::*;
use helpers::parser_parent::parser_parent_index;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/ActionControllerFlashBeforeRender";
pub static COP: ActionControllerFlashBeforeRender = ActionControllerFlashBeforeRender;
pub struct ActionControllerFlashBeforeRender;

/// (send nil? :flash)
fn flash(n: &Node) -> bool {
    helpers::rails::send_node(n).is_some_and(|c| helpers::rails::is_command(&c, "flash") && send_args(&c).is_empty())
}

/// The parser node at ancestor `k` as a range (a BlockNode stands for its call with the block).
fn range_at(anc: &[Node], k: usize) -> (usize, usize) {
    if anc[k].as_block_node().is_some() && k > 0 {
        return (anc[k - 1].location().start_offset(), anc[k].location().end_offset());
    }
    (anc[k].location().start_offset(), anc[k].location().end_offset())
}

/// The parser right siblings of the node at ancestor `k`, for the parents this cop meets: a
/// statement list, an if's branches; none otherwise.
// ponytail: siblings under other parent types (sends, hashes, ...) are not modelled.
fn right_siblings<'a, 'pr>(anc: &'a [Node<'pr>], k: usize) -> Vec<Node<'pr>> {
    let me = range_at(anc, k);
    let Some(p) = parser_parent_index(anc, k) else { return Vec::new() };
    let r = |n: &Node| (n.location().start_offset(), n.location().end_offset());
    let after = |nodes: Vec<Node<'pr>>| -> Vec<Node<'pr>> {
        match nodes.iter().position(|n| r(n) == me) {
            Some(i) => nodes.into_iter().skip(i + 1).collect(),
            None => Vec::new(),
        }
    };
    let parent = &anc[p];
    if let Some(s) = parent.as_statements_node() {
        return after(s.body().iter().collect());
    }
    if let Some(pn) = parent.as_parentheses_node() {
        return after(helpers::rails::body_statements(pn.body()));
    }
    if parent.as_if_node().is_some() || parent.as_unless_node().is_some() {
        let Some((c, t, f)) = helpers::rails::if_children(parent) else { return Vec::new() };
        let kids: Vec<Option<Node>> = vec![Some(c), t, f];
        let Some(i) = kids.iter().position(|x| x.as_ref().is_some_and(|x| r(x) == me)) else { return Vec::new() };
        return kids.into_iter().skip(i + 1).flatten().collect();
    }
    Vec::new()
}

/// use_redirect_to?: a right sibling is a `redirect_to` send (or `return redirect_to ...`).
fn use_redirect_to(sibs: &[Node]) -> bool {
    sibs.iter().any(|s| {
        let target: Node = match s.as_return_node().and_then(|rt| rt.arguments()) {
            Some(a) if a.arguments().iter().count() == 1 => a.arguments().iter().next().unwrap(),
            _ => match s.as_return_node() {
                Some(_) => return false,
                None => {
                    return helpers::rails::send_node(s).is_some_and(|c| method_name(&c) == "redirect_to");
                }
            },
        };
        helpers::rails::send_node(&target).is_some_and(|c| method_name(&c) == "redirect_to")
    })
}

/// render?: a receiver-less `render` send anywhere in the node.
fn has_render(n: &Node) -> bool {
    struct R(bool);
    impl<'pr> Visit<'pr> for R {
        fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
            if !is_csend(c) && helpers::rails::is_command(c, "render") {
                self.0 = true;
            }
            ruby_prism::visit_call_node(self, c);
        }
    }
    let mut r = R(false);
    r.visit(n);
    r.0
}

impl Cop for ActionControllerFlashBeforeRender {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        if !flash(node) {
            return;
        }
        let anc = cx.ancestors;
        // flash_assignment?: the parent is `flash[...] = ...`.
        let Some(ai) = parser_parent_index(anc, anc.len()) else { return };
        let Some(asgn) = helpers::rails::send(&anc[ai]) else { return };
        if method_name(&asgn) != "[]=" || !asgn.receiver().is_some_and(|r| flash(&r)) {
            return;
        }
        // followed_by_render?
        let followed = {
            // context.each_ancestor(:if, :rescue).first
            let cond_or_rescue = (0..ai).rev().find(|&k| {
                matches!(anc[k], Node::IfNode { .. } | Node::UnlessNode { .. } | Node::RescueModifierNode { .. })
                    && !is_pattern_guard(&anc[k], &anc[..k])
                    || anc[k].as_begin_node().is_some_and(|b| b.rescue_clause().is_some())
            });
            if let Some(k) = cond_or_rescue {
                if use_redirect_to(&right_siblings(anc, ai)) {
                    false
                } else {
                    let sibs = if let Some(b) = anc[k].as_begin_node() {
                        // A rescue inside `ensure`: the ensure body follows it.
                        b.ensure_clause().and_then(|e| e.statements()).map(|s| s.body().iter().collect()).unwrap_or_default()
                    } else {
                        right_siblings(anc, k)
                    };
                    sibs.iter().any(has_render)
                }
            } else {
                let sibs = right_siblings(anc, ai);
                let parent_redirect = parser_parent_index(anc, ai).is_some_and(|p| use_redirect_to(&right_siblings(anc, p)));
                if sibs.is_empty() && !parent_redirect { true } else { sibs.iter().any(has_render) }
            }
        };
        if !followed {
            return;
        }
        // instance_method_or_block?
        let in_method = anc.iter().any(|a| {
            a.as_def_node().is_some_and(|d| d.receiver().is_none())
                || a.as_block_node().is_some_and(|b| {
                    !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
                })
                || a.as_lambda_node().is_some_and(|l| {
                    !l.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
                })
        });
        if !in_method {
            return;
        }
        // inherit_action_controller_base?: the nearest class mentions the base classes anywhere.
        let Some(class) = anc.iter().rev().find(|a| a.as_class_node().is_some()) else { return };
        struct C(bool);
        impl<'pr> Visit<'pr> for C {
            fn visit_constant_read_node(&mut self, n: &ruby_prism::ConstantReadNode<'pr>) {
                self.0 |= n.name().as_slice() == b"ApplicationController";
            }
            fn visit_constant_path_node(&mut self, n: &ruby_prism::ConstantPathNode<'pr>) {
                let node = n.as_node();
                self.0 |= helpers::parser_parent::is_top_const(&node, "ApplicationController")
                    || helpers::rails::is_const(&node, "ActionController::Base");
                ruby_prism::visit_constant_path_node(self, n);
            }
        }
        let mut c = C(false);
        c.visit(class);
        if !c.0 {
            return;
        }
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        cx.add_fix(s, e, "Use `flash.now` before `render`.", |c| c.replace(s, e, "flash.now"));
    }
}
