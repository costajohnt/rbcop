use crate::prelude::*;
use helpers::variable_force::{self as vf, T, Tree};

pub const NAME: &str = "Lint/ShadowingOuterLocalVariable";
pub static COP: ShadowingOuterLocalVariable = ShadowingOuterLocalVariable;
pub struct ShadowingOuterLocalVariable;

impl Cop for ShadowingOuterLocalVariable {
    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let tree = Tree::build(&node.as_program_node().unwrap(), cx.src);
        let text = cx.src.text;
        let mut found: Vec<(usize, usize, String)> = Vec::new();
        vf::investigate_with(
            &tree,
            |tree, var, outer| {
                let Some(outer) = outer else { return };
                if var.name.starts_with('_') || ractor_block(tree, text, var.scope.node) {
                    return;
                }
                let eq = |a: usize, b: usize| same(tree, text, a, b);
                // variable_used_in_declaration_of_outer?
                if tree.ancestors(var.scope.node).any(|a| eq(a, outer.declaration)) {
                    return;
                }
                if same_conditions_node_different_branch(tree, &eq, var.scope.node, outer.declaration) {
                    return;
                }
                let (s, e) = tree.range(var.declaration);
                found.push((s, e, var.name.clone()));
            },
            |_, _, _| {},
        );
        for (s, e, name) in found {
            cx.add(s, e, format!("Shadowing outer local variable - `{name}`."));
        }
    }
}

/// rubocop-ast structural `==`, approximated by type and source.
fn same(tree: &Tree, text: &str, a: usize, b: usize) -> bool {
    if a == b {
        return true;
    }
    let (na, nb) = (&tree.nodes[a], &tree.nodes[b]);
    na.t == nb.t && na.kind == nb.kind && text[na.start..na.end] == text[nb.start..nb.end]
}

/// `(block (send (const nil? :Ractor) :new ...) ...)`
fn ractor_block(tree: &Tree, text: &str, scope: usize) -> bool {
    if tree.t(scope) != T::Block {
        return false;
    }
    let Some(send) = tree.child(scope, 0).filter(|&s| tree.t(s) == T::Send && tree.nodes[s].name == "new") else {
        return false;
    };
    tree.child(send, 0).is_some_and(|r| {
        let n = &tree.nodes[r];
        n.kind == kind::CONSTANT_READ_NODE && &text[n.start..n.end] == "Ractor"
    })
}

fn conditional(tree: &Tree, n: usize) -> bool {
    matches!(tree.t(n), T::If | T::While | T::Until | T::Case | T::CaseMatch)
}

fn in_pattern(tree: &Tree, n: usize) -> Option<usize> {
    tree.ancestors(n).find(|&a| tree.t(a) == T::Other && tree.nodes[a].kind == kind::IN_NODE)
}

fn same_conditions_node_different_branch(tree: &Tree, eq: &impl Fn(usize, usize) -> bool, scope: usize, outer: usize) -> bool {
    // different_case_in_branch?
    if let (Some(inner), Some(outer_branch)) = (in_pattern(tree, scope), in_pattern(tree, outer))
        && !eq(inner, outer_branch)
    {
        let same_parent = match (tree.parent(inner), tree.parent(outer_branch)) {
            (Some(a), Some(b)) => eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same_parent {
            return true;
        }
    }
    // variable_node: the scope's parent (a `when`'s case). RuboCop raises on a parentless scope.
    let Some(mut variable_node) = tree.parent(scope) else { return true };
    if tree.t(variable_node) == T::Other && tree.nodes[variable_node].kind == kind::WHEN_NODE {
        let Some(p) = tree.parent(variable_node) else { return true };
        variable_node = p;
    }
    if !conditional(tree, variable_node) && !tree.ancestors(variable_node).any(|a| conditional(tree, a)) {
        return false;
    }
    let Some(outer_cond) = tree.ancestors(outer).find(|&a| conditional(tree, a)) else { return false };
    if eq(variable_node, outer_cond) {
        return true;
    }
    tree.t(outer_cond) == T::If
        && tree.child(outer_cond, if tree.nodes[outer_cond].unless { 1 } else { 2 }).is_some_and(|e| eq(variable_node, e))
}
