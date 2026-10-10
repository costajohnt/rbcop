//! RuboCop's VariableForce (scopes, variables, assignments, references, branches), run over the
//! parser-shaped tree from `parser_tree`. `VariableForce::run` returns the scopes in the order
//! RuboCop's `after_leaving_scope` hooks see them; cops then inspect `scopes[i].vars`.

use super::parser_tree::{Id, Tree, T};
use std::collections::{HashMap, HashSet};

pub struct Scope {
    pub node: Id,
    pub naked: bool,
    /// Variables in declaration order (Ruby Hash order); `by_name` indexes into `vf.vars`.
    pub vars: Vec<usize>,
    by_name: HashMap<String, usize>,
}

pub struct Variable {
    pub name: String,
    pub decl: Id,
    /// Index into `vf.scopes`.
    pub scope: usize,
    pub assignments: Vec<usize>,
    pub references: Vec<Reference>,
    pub captured: bool,
}

pub struct Assignment {
    pub node: Id,
    pub var: usize,
    pub referenced: bool,
    pub reassigned: bool,
    pub branch: Option<Branch>,
}

pub struct Reference {
    pub node: Id,
    pub branch: Option<Branch>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Branch {
    pub child: Id,
    pub control: Id,
}

pub struct VariableForce<'t> {
    pub tree: &'t Tree,
    pub scopes: Vec<Scope>,
    pub vars: Vec<Variable>,
    pub assignments: Vec<Assignment>,
    stack: Vec<usize>,
    scanned: HashSet<Id>,
    /// Scopes in the order they were left.
    pub left: Vec<usize>,
}

const LOOP_TYPES: [T; 5] = [T::While, T::Until, T::WhilePost, T::UntilPost, T::For];

pub fn is_scope_type(t: T) -> bool {
    matches!(t, T::Block | T::Numblock | T::Itblock | T::Class | T::Sclass | T::Defs | T::Module | T::Def)
}

pub fn is_argument_type(t: T) -> bool {
    matches!(t, T::Arg | T::Optarg | T::Restarg | T::Kwarg | T::Kwoptarg | T::Kwrestarg | T::Blockarg | T::Shadowarg)
}

fn any_block(t: T) -> bool {
    matches!(t, T::Block | T::Numblock | T::Itblock)
}

/// `Scope::OUTER_SCOPE_CHILD_INDICES`.
fn outer_indices(t: T) -> Option<std::ops::RangeInclusive<usize>> {
    match t {
        T::Defs | T::Module | T::Sclass | T::Block => Some(0..=0),
        T::Class => Some(0..=1),
        _ => None,
    }
}

impl<'t> VariableForce<'t> {
    pub fn run(tree: &'t Tree) -> Self {
        let mut vf = VariableForce {
            tree,
            scopes: Vec::new(),
            vars: Vec::new(),
            assignments: Vec::new(),
            stack: Vec::new(),
            scanned: HashSet::new(),
            left: Vec::new(),
        };
        if let Some(root) = tree.root {
            vf.push_scope(root);
            vf.process_node(root);
            vf.pop_scope();
        }
        vf
    }

    // ---- Scope ----

    /// `Scope#include?`.
    pub fn scope_include(&self, scope: usize, target: Id) -> bool {
        !self.belong_to_outer(scope, target) && !self.belong_to_inner(scope, target)
    }

    fn belong_to_outer(&self, scope: usize, target: Id) -> bool {
        let s = &self.scopes[scope];
        let t = self.tree;
        if !s.naked && target == s.node {
            return true;
        }
        if t.ancestors(s.node).any(|a| a == target) {
            return true;
        }
        if t.parent(target) != Some(s.node) {
            return false;
        }
        match outer_indices(t.t(s.node)) {
            Some(r) => t.sibling_index(target).is_some_and(|i| r.contains(&i)),
            None => false,
        }
    }

    fn belong_to_inner(&self, scope: usize, target: Id) -> bool {
        let t = self.tree;
        let Some(p) = t.parent(target) else { return false };
        if p == self.scopes[scope].node || !is_scope_type(t.t(p)) {
            return false;
        }
        match outer_indices(t.t(p)) {
            Some(r) => !t.sibling_index(target).is_some_and(|i| r.contains(&i)),
            None => true,
        }
    }

    /// `Scope#each_node`.
    pub fn scope_nodes(&self, scope: usize) -> Vec<Id> {
        let mut out = Vec::new();
        let s = &self.scopes[scope];
        if s.naked {
            out.push(s.node);
        }
        self.scan(scope, s.node, &mut out);
        out
    }

    fn scan(&self, scope: usize, node: Id, out: &mut Vec<Id>) {
        for c in self.tree.child_nodes(node) {
            if !self.scope_include(scope, c) {
                continue;
            }
            out.push(c);
            self.scan(scope, c, out);
        }
    }

    /// `Scope#body_node`.
    pub fn body_node(&self, scope: usize) -> Option<Id> {
        let s = &self.scopes[scope];
        if s.naked {
            return Some(s.node);
        }
        let i = match self.tree.t(s.node) {
            T::Module | T::Sclass => 1,
            T::Defs => 3,
            _ => 2,
        };
        self.tree.kid(s.node, i)
    }

    // ---- Branch ----

    /// `Branch.of(node, scope:)`.
    pub fn branch_of(&self, target: Id, scope: usize) -> Option<Branch> {
        let t = self.tree;
        for node in std::iter::once(target).chain(t.ancestors(target)) {
            let parent = t.parent(node)?;
            if !self.scope_include(scope, node) {
                return None;
            }
            let b = Branch { child: node, control: parent };
            let Some(always) = self.always_run(b) else { continue };
            if !always {
                return Some(b);
            }
        }
        None
    }

    /// `always_run?` of the branch class for the control node's type; None if it has none.
    fn always_run(&self, b: Branch) -> Option<bool> {
        let t = self.tree;
        let first = t.kid(b.control, 0) == Some(b.child);
        Some(match t.t(b.control) {
            T::If | T::While | T::Until | T::WhilePost | T::UntilPost | T::Case | T::CaseMatch => first,
            T::And | T::Or | T::AndAsgn | T::OrAsgn | T::OpAsgn => first,
            T::For => first || t.kid(b.control, 1) == Some(b.child),
            T::Rescue => false,
            T::Ensure => t.nodes[b.control].kids.last().copied().flatten() == Some(b.child),
            _ => return None,
        })
    }

    /// ExceptionHandler `main_body?` (Rescue, Ensure): may jump / may run incompletely.
    fn main_body(&self, b: Branch) -> bool {
        matches!(self.tree.t(b.control), T::Rescue | T::Ensure) && self.tree.kid(b.control, 0) == Some(b.child)
    }

    fn branch_parent(&self, b: Branch, scope: usize) -> Option<Branch> {
        self.branch_of(b.control, scope)
    }

    fn exclusive_with(&self, b: Branch, other: Option<Branch>, scope: usize, other_scope: usize) -> bool {
        let Some(other) = other else { return false };
        if self.main_body(b) {
            return false;
        }
        let mut cur = Some(other);
        while let Some(o) = cur {
            if b.control == o.control {
                return b.child != o.child;
            }
            cur = self.branch_parent(o, other_scope);
        }
        match self.branch_parent(b, scope) {
            Some(p) => self.exclusive_with(p, Some(other), scope, other_scope),
            None => false,
        }
    }

    // ---- Variable table ----

    fn push_scope(&mut self, node: Id) {
        let naked = !is_scope_type(self.tree.t(node));
        self.scopes.push(Scope { node, naked, vars: Vec::new(), by_name: HashMap::new() });
        self.stack.push(self.scopes.len() - 1);
    }

    fn pop_scope(&mut self) {
        let s = self.stack.pop().unwrap();
        self.left.push(s);
    }

    fn current(&self) -> usize {
        *self.stack.last().unwrap()
    }

    fn declare_variable(&mut self, name: &str, node: Id) {
        let scope = self.current();
        let v = self.vars.len();
        self.vars.push(Variable {
            name: name.to_string(),
            decl: node,
            scope,
            assignments: Vec::new(),
            references: Vec::new(),
            captured: false,
        });
        let s = &mut self.scopes[scope];
        match s.by_name.insert(name.to_string(), v) {
            Some(old) => {
                let pos = s.vars.iter().position(|&x| x == old).unwrap();
                s.vars[pos] = v;
            }
            None => s.vars.push(v),
        }
    }

    pub fn find_variable(&self, name: &str) -> Option<usize> {
        for &s in self.stack.iter().rev() {
            if let Some(&v) = self.scopes[s].by_name.get(name) {
                return Some(v);
            }
            if !any_block(self.tree.t(self.scopes[s].node)) {
                return None;
            }
        }
        None
    }

    fn accessible_variables(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for &s in self.stack.iter().rev() {
            out.extend(self.scopes[s].vars.iter().copied());
            if !any_block(self.tree.t(self.scopes[s].node)) {
                break;
            }
        }
        out
    }

    fn mark_captured(&mut self, v: usize) {
        let cur = self.current();
        if any_block(self.tree.t(self.scopes[cur].node)) && self.vars[v].scope != cur {
            self.vars[v].captured = true;
        }
    }

    fn assign_to_variable(&mut self, name: &str, node: Id) {
        let Some(v) = self.find_variable(name) else { return };
        self.mark_captured(v);
        self.assign(v, node);
    }

    fn reference_variable(&mut self, name: &str, node: Id) {
        let Some(v) = self.find_variable(name) else { return };
        self.mark_captured(v);
        self.reference(v, node);
    }

    // ---- Variable ----

    fn assign(&mut self, v: usize, node: Id) {
        let scope = self.vars[v].scope;
        let branch = self.branch_of(node, scope);
        let a = self.assignments.len();
        self.assignments.push(Assignment { node, var: v, referenced: false, reassigned: false, branch });
        if !self.vars[v].captured {
            if let Some(&last) = self.vars[v].assignments.last() {
                if self.assignments[last].branch == branch && !self.assignments[last].referenced {
                    self.assignments[last].reassigned = true;
                }
            }
        }
        self.vars[v].assignments.push(a);
    }

    fn reference(&mut self, v: usize, node: Id) {
        let scope = self.vars[v].scope;
        let rbranch = self.branch_of(node, scope);
        self.vars[v].references.push(Reference { node, branch: rbranch });
        let mut consumed: Vec<Branch> = Vec::new();
        let list: Vec<usize> = self.vars[v].assignments.iter().rev().copied().collect();
        for a in list {
            let abranch = self.assignments[a].branch;
            if abranch.is_some_and(|b| consumed.contains(&b)) {
                continue;
            }
            let exclusive = match (abranch, rbranch) {
                (Some(ab), Some(_)) => self.exclusive_with(ab, rbranch, scope, scope),
                _ => false,
            };
            if !exclusive {
                self.assignments[a].referenced = true;
            }
            if self.in_modifier_conditional(self.assignments[a].node, node) {
                continue;
            }
            let Some(ab) = abranch else { break };
            if Some(ab) == rbranch {
                break;
            }
            if !self.main_body(ab) {
                consumed.push(ab);
            }
        }
    }

    fn in_modifier_conditional(&self, asg: Id, reference: Id) -> bool {
        let t = self.tree;
        let cond = t.ancestors(asg).find(|&c| {
            matches!(t.t(c), T::If | T::While | T::Until)
                && t.nodes[c].modifier
                && t.kid(c, 0).is_some_and(|k| t.covers(k, asg))
        });
        let Some(c) = cond else { return false };
        t.covers(c, reference) && !t.kid(c, 0).is_some_and(|k| t.covers(k, reference))
    }

    // ---- Processing ----

    fn process_node(&mut self, node: Id) {
        if !self.handle(node) {
            self.process_children(node);
        }
    }

    fn process_children(&mut self, node: Id) {
        let kids: Vec<Id> = self.tree.child_nodes(node).collect();
        for k in kids {
            if !self.scanned.contains(&k) {
                self.process_node(k);
            }
        }
    }

    /// Node handler; true means skip children.
    fn handle(&mut self, node: Id) -> bool {
        let tree = self.tree;
        let n = &tree.nodes[node];
        match n.t {
            T::Lvasgn => {
                if self.find_variable(&n.name).is_none() {
                    self.declare_variable(&n.name, node);
                }
                self.process_children(node);
                self.assign_to_variable(&n.name, node);
                true
            }
            T::MatchWithLvasgn => {
                for name in &n.names {
                    if self.find_variable(name).is_none() {
                        self.declare_variable(name, node);
                    }
                }
                if let Some(rhs) = tree.kid(node, 1) {
                    self.process_node(rhs);
                }
                if let Some(re) = tree.kid(node, 0) {
                    self.process_node(re);
                }
                for name in &n.names {
                    self.assign_to_variable(name, node);
                }
                true
            }
            T::MatchVar => {
                if self.find_variable(&n.name).is_none() {
                    self.declare_variable(&n.name, node);
                }
                true
            }
            T::Masgn => {
                if let Some(rhs) = tree.kid(node, 1) {
                    self.process_node(rhs);
                }
                if let Some(lhs) = tree.kid(node, 0) {
                    self.process_node(lhs);
                }
                true
            }
            T::Lvar => {
                self.reference_variable(&n.name, node);
                false
            }
            T::Rescue => {
                let retry = tree
                    .child_nodes(node)
                    .filter(|&c| tree.t(c) == T::Resbody)
                    .any(|c| tree.descendants(c).iter().any(|&d| tree.t(d) == T::Retry));
                if retry {
                    self.process_loop(node);
                    return true;
                }
                false
            }
            T::Zsuper => {
                for v in self.accessible_variables() {
                    let s = self.vars[v].scope;
                    let method_arg = is_argument_type(tree.t(self.vars[v].decl))
                        && matches!(tree.t(self.scopes[s].node), T::Def | T::Defs);
                    if method_arg {
                        self.reference(v, node);
                    }
                }
                false
            }
            T::Send => {
                if n.name == "binding" && tree.kid(node, 2).is_none_or(|a| tree.nodes[a].childless) {
                    for v in self.accessible_variables() {
                        self.reference(v, node);
                    }
                }
                false
            }
            t if is_argument_type(t) => {
                if !n.name.is_empty() {
                    self.declare_variable(&n.name, node);
                }
                false
            }
            T::OpAsgn | T::OrAsgn | T::AndAsgn => {
                let Some(lhs) = tree.kid(node, 0).filter(|&l| tree.t(l) == T::Lvasgn) else { return false };
                let name = &tree.nodes[lhs].name;
                if self.find_variable(name).is_none() {
                    self.declare_variable(name, lhs);
                }
                self.reference_variable(name, node);
                if let Some(rhs) = n.kids.last().copied().flatten() {
                    self.process_node(rhs);
                }
                self.assign_to_variable(name, lhs);
                true
            }
            t if LOOP_TYPES.contains(&t) => {
                self.process_loop(node);
                true
            }
            t if is_scope_type(t) => {
                if !matches!(t, T::Def) {
                    let mut twisted = vec![tree.kid(node, 0)];
                    if t == T::Class {
                        twisted.push(tree.kid(node, 1));
                    }
                    for tw in twisted.into_iter().flatten() {
                        self.process_node(tw);
                        self.scanned.insert(tw);
                    }
                }
                self.push_scope(node);
                self.process_children(node);
                self.pop_scope();
                true
            }
            _ => false,
        }
    }

    fn process_loop(&mut self, node: Id) {
        let tree = self.tree;
        match tree.t(node) {
            T::WhilePost | T::UntilPost => {
                if let Some(b) = tree.kid(node, 1) {
                    self.process_node(b);
                }
                if let Some(c) = tree.kid(node, 0) {
                    self.process_node(c);
                }
            }
            T::For => {
                for i in [1, 0, 2] {
                    if let Some(k) = tree.kid(node, i) {
                        self.process_node(k);
                    }
                }
            }
            _ => self.process_children(node),
        }
        self.mark_assignments_as_referenced_in_loop(node);
    }

    fn mark_assignments_as_referenced_in_loop(&mut self, node: Id) {
        let tree = self.tree;
        let mut names: Vec<&str> = Vec::new();
        let mut asg_nodes: HashSet<Id> = HashSet::new();
        for d in tree.descendants(node) {
            match tree.t(d) {
                T::Lvar => names.push(&tree.nodes[d].name),
                T::Lvasgn => {
                    asg_nodes.insert(d);
                }
                T::OpAsgn | T::OrAsgn | T::AndAsgn => {
                    if let Some(l) = tree.kid(d, 0).filter(|&l| tree.t(l) == T::Lvasgn) {
                        names.push(&tree.nodes[l].name);
                    }
                }
                _ => {}
            }
        }
        for name in names {
            let Some(v) = self.find_variable(name) else { continue };
            let loop_asgs: Vec<usize> =
                self.vars[v].assignments.iter().copied().filter(|&a| asg_nodes.contains(&self.assignments[a].node)).collect();
            if loop_asgs.is_empty() {
                continue;
            }
            for &a in &loop_asgs {
                let an = self.assignments[a].node;
                if tree.ancestors(an).any(|x| matches!(tree.t(x), T::If | T::Case | T::CaseMatch | T::Rescue)) {
                    self.assignments[a].referenced = true;
                }
            }
            self.assignments[*loop_asgs.last().unwrap()].referenced = true;
        }
    }

    // ---- Queries used by cops ----

    /// `Assignment#used?`.
    pub fn assignment_used(&self, a: usize) -> bool {
        let asg = &self.assignments[a];
        (!asg.reassigned && self.vars[asg.var].captured) || asg.referenced
    }

    /// `Variable#argument?`.
    pub fn is_argument(&self, v: usize) -> bool {
        is_argument_type(self.tree.t(self.vars[v].decl))
    }
}
