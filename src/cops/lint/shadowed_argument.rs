use crate::helpers::variable_force::{self as vf, Tree, Variable, T};
use crate::prelude::*;

pub const NAME: &str = "Lint/ShadowedArgument";
pub static COP: ShadowedArgument = ShadowedArgument;
pub struct ShadowedArgument;

/// `Assignment#meta_assignment_node`: the op-assign / masgn / splat / for node around an lvasgn.
fn meta_assignment_node(tree: &Tree, n: usize) -> Option<usize> {
    let parent = tree.parent(n)?;
    if tree.t(parent).is_shorthand_asgn() && tree.sibling_index(n) == Some(0) {
        return Some(parent);
    }
    // multiple_assignment_node
    let mut cand = parent;
    if tree.t(cand) == T::Splat {
        cand = tree.parent(cand).unwrap_or(cand);
    }
    if tree.t(cand) == T::Mlhs
        && let Some(grand) = tree.parent(parent)
    {
        let mut g = grand;
        while tree.t(g) == T::Mlhs {
            let Some(p) = tree.parent(g) else { break };
            if tree.t(p) == T::Masgn {
                return Some(p);
            }
            g = p;
        }
        if tree.t(grand) == T::Masgn {
            return Some(grand);
        }
    }
    if tree.t(parent) == T::Splat {
        return Some(parent);
    }
    if tree.t(parent) == T::For {
        return Some(parent);
    }
    let grand = tree.parent(parent)?;
    (tree.t(parent) == T::Mlhs && tree.t(grand) == T::For).then_some(grand)
}

fn conditional_assignment(tree: &Tree, node: usize, stop: usize) -> bool {
    let mut node = Some(node);
    while let Some(n) = node {
        if n == stop {
            return false;
        }
        if matches!(tree.t(n), T::If | T::While | T::Until | T::Case | T::CaseMatch | T::Block | T::Rescue) {
            return true;
        }
        node = tree.parent(n);
    }
    false
}

fn uses_var(tree: &Tree, node: usize, name: &str) -> bool {
    let mut found = false;
    tree.each_node(node, &mut |n| found |= tree.t(n) == T::Lvar && tree.nodes[n].name == name);
    found
}

fn reference_pos(tree: &Tree, node: usize) -> usize {
    match tree.parent(node) {
        Some(p) if tree.t(p) == T::Masgn => tree.nodes[p].start,
        _ => tree.nodes[node].start,
    }
}

/// `shadowing_assignment`: the node to report for a shadowed argument, if any.
fn shadowing_node(tree: &Tree, arg: &Variable, ignore_implicit: bool) -> Option<usize> {
    if arg.references.is_empty() {
        return None;
    }
    let mut location_known = true;
    let mut found = None;
    for a in &arg.assignments {
        let node = meta_assignment_node(tree, a.node).unwrap_or(a.node);
        if tree.t(node).is_shorthand_asgn() || tree.parent(node).is_none() {
            location_known = false;
            continue;
        }
        let conditional = conditional_assignment(tree, tree.parent(node).unwrap(), arg.scope.node);
        if !uses_var(tree, node, &arg.name) {
            if conditional {
                location_known = false;
                continue;
            }
            found = Some((a.node, location_known));
            break;
        }
    }
    let (node, known) = found?;
    let pos = tree.nodes[node].start;
    let assignment_refs: Vec<(usize, usize)> =
        arg.assignments.iter().flat_map(|a| a.references.iter().map(|&r| tree.range(r))).collect();
    let before = arg
        .references
        .iter()
        .filter(|r| !(r.explicit(tree) && assignment_refs.contains(&tree.range(r.node))))
        .any(|r| (!r.explicit(tree) && ignore_implicit) || reference_pos(tree, r.node) <= pos);
    if before {
        return None;
    }
    Some(if known { node } else { arg.declaration })
}

impl Cop for ShadowedArgument {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let tree = Tree::build(&node.as_program_node().unwrap(), cx.src);
        let ignore_implicit = cx.cfg.bool("IgnoreImplicitReferences") == Some(true);
        let mut offenses = Vec::new();
        vf::investigate(&tree, |tree, scope, vars| {
            for &v in &scope.variables {
                let arg = &vars[v];
                if !(arg.is_method_argument(tree) || arg.is_block_argument(tree)) || tree.t(arg.declaration) == T::Shadowarg {
                    continue;
                }
                if let Some(n) = shadowing_node(tree, arg, ignore_implicit) {
                    offenses.push((tree.range(n), arg.name.clone()));
                }
            }
        });
        for ((s, e), name) in offenses {
            cx.add(s, e, format!("Argument `{name}` was shadowed by a local variable before it was used."));
        }
    }
}
