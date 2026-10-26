use crate::prelude::*;
use helpers::variable_force::{self as vf, ScopeRef, T, Tree};

pub const NAME: &str = "Lint/UselessAssignment";
pub static COP: UselessAssignment = UselessAssignment;
pub struct UselessAssignment;

type Id = usize;

/// rubocop-ast `assignment?`.
fn is_assignment(tree: &Tree, n: Id) -> bool {
    match tree.t(n) {
        T::Lvasgn | T::Masgn | T::OpAsgn | T::OrAsgn | T::AndAsgn => true,
        T::Other => matches!(
            tree.nodes[n].kind,
            kind::INSTANCE_VARIABLE_WRITE_NODE
                | kind::CLASS_VARIABLE_WRITE_NODE
                | kind::GLOBAL_VARIABLE_WRITE_NODE
                | kind::CONSTANT_WRITE_NODE
                | kind::CONSTANT_PATH_WRITE_NODE
                | kind::INSTANCE_VARIABLE_TARGET_NODE
                | kind::CLASS_VARIABLE_TARGET_NODE
                | kind::GLOBAL_VARIABLE_TARGET_NODE
                | kind::CONSTANT_TARGET_NODE
                | kind::CONSTANT_PATH_TARGET_NODE
        ),
        _ => false,
    }
}

/// `Assignment#meta_assignment_node` with its kind.
#[derive(Clone, Copy, PartialEq)]
enum Meta {
    Operator(Id),
    Multiple,
    Rest,
    For,
}

fn meta(tree: &Tree, node: Id) -> Option<Meta> {
    let parent = tree.parent(node)?;
    // operator_assignment_node
    if tree.t(parent).is_shorthand_asgn() && tree.sibling_index(node) == Some(0) {
        return Some(Meta::Operator(parent));
    }
    // multiple_assignment_node
    let cand = if tree.t(parent) == T::Splat { tree.parent(parent) } else { Some(parent) };
    if cand.is_some_and(|c| tree.t(c) == T::Mlhs)
        && let Some(gp) = tree.parent(parent)
    {
        let mut g = gp;
        while tree.t(g) == T::Mlhs {
            let Some(p) = tree.parent(g) else { break };
            if tree.t(p) == T::Masgn {
                return Some(Meta::Multiple);
            }
            g = p;
        }
        if tree.t(gp) == T::Masgn {
            return Some(Meta::Multiple);
        }
    }
    // rest_assignment_node
    if tree.t(parent) == T::Splat {
        return Some(Meta::Rest);
    }
    // for_assignment_node
    if tree.t(parent) == T::For || (tree.t(parent) == T::Mlhs && tree.parent(parent).is_some_and(|g| tree.t(g) == T::For)) {
        return Some(Meta::For);
    }
    None
}

fn descendants(tree: &Tree, n: Id) -> Vec<Id> {
    let mut out = Vec::new();
    tree.each_node(n, &mut |d| {
        if d != n {
            out.push(d)
        }
    });
    out
}

fn sequential_assignment(tree: &Tree, node: Id) -> bool {
    std::iter::once(node).chain(tree.ancestors(node)).any(|n| {
        tree.t(n) == T::Lvasgn
            && tree.child(n, 0).is_some_and(|e| tree.nodes[e].kind == kind::ARRAY_NODE)
            && descendants(tree, n).into_iter().any(|d| is_assignment(tree, d))
    })
}

fn variable_in_loop_condition(tree: &Tree, node: Id, name: &str) -> bool {
    // A method defined inside the loop has its own scope, so the loop condition can only read
    // the variable if the loop is nearer than any method definition (1.92).
    let Some(lp) = tree.ancestors(node).find(|&a| {
        matches!(tree.t(a), T::While | T::Until | T::WhilePost | T::UntilPost | T::For | T::Def | T::Defs)
    }) else {
        return false;
    };
    if matches!(tree.t(lp), T::For | T::Def | T::Defs) {
        return false;
    }
    let Some(cond) = tree.child(lp, 0) else { return false };
    let mut found = false;
    tree.each_node(cond, &mut |n| found |= tree.t(n) == T::Lvar && tree.nodes[n].name == name);
    found
}

/// `Scope#body_node`.
fn body_node(tree: &Tree, scope: ScopeRef) -> Option<Id> {
    if scope.naked {
        return Some(scope.node);
    }
    let i = match tree.t(scope.node) {
        T::Module | T::Sclass => 1,
        T::Defs => 3,
        _ => 2,
    };
    tree.child(scope.node, i)
}

/// `Scope#each_node`.
fn scope_nodes(tree: &Tree, scope: ScopeRef, n: Id, out: &mut Vec<Id>) {
    for &c in tree.nodes[n].children.iter().flatten() {
        if tree.scope_include(scope, c) {
            out.push(c);
            scope_nodes(tree, scope, c, out);
        }
    }
}

/// `loc.operator` of an op-assign node whose left side ends at `lhs_end`.
fn operator(src: &str, lhs_end: usize, value_start: usize) -> Option<(usize, usize)> {
    let between = &src[lhs_end..value_start];
    let start = lhs_end + between.find(|c: char| !helpers::ruby_string::is_space(c))?;
    let end = lhs_end + between.find('=')? + 1;
    Some((start, end))
}

/// rubocop-ast ArrayNode#bracketed? of the array at `range` (`[...]` or a `%w`-style literal).
fn bracketed(program: &Node, range: (usize, usize)) -> bool {
    struct Find {
        range: (usize, usize),
        found: Option<bool>,
    }
    impl<'pr> ruby_prism::Visit<'pr> for Find {
        fn visit_array_node(&mut self, node: &ruby_prism::ArrayNode<'pr>) {
            let l = node.location();
            if (l.start_offset(), l.end_offset()) == self.range {
                self.found.get_or_insert(node.opening_loc().is_some());
            }
            ruby_prism::visit_array_node(self, node);
        }
    }
    let mut f = Find { range, found: None };
    ruby_prism::Visit::visit(&mut f, program);
    f.found.unwrap_or(true)
}

enum Fix {
    /// Replace `start..value` with `[` and insert `]` at `end`.
    Bracket(usize, usize, usize),
    None,
    Remove(usize, usize),
    Replace(usize, usize, String),
}

impl Cop for UselessAssignment {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::PROGRAM_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let tree = Tree::shared(&node.as_program_node().unwrap(), cx.src);
        let src = cx.src.text;
        let mut ignored: Vec<(usize, usize)> = Vec::new();
        let mut offenses: Vec<(usize, usize, String, Fix)> = Vec::new();
        vf::investigate(tree, |tree, scope, vars| {
            // The did-you-mean candidates of a scope, built once: rebuilding them per useless
            // assignment is cubic (20k `x = 1` lines never finished).
            let mut scope_names: Option<(Id, Vec<&str>)> = None;
            for &v in &scope.variables {
                let var = &vars[v];
                if var.name.starts_with('_') {
                    continue;
                }
                for a in var.assignments.iter().rev() {
                    let an = a.node;
                    let (as_, ae) = tree.range(an);
                    let used = (!a.reassigned && var.captured_by_block && !var.references.is_empty()) || a.referenced;
                    if used
                        || ignored.iter().any(|&(s, e)| s <= as_ && e >= ae)
                        || variable_in_loop_condition(tree, an, &var.name)
                    {
                        continue;
                    }
                    let m = meta(tree, an);
                    let mut msg = format!("Useless assignment to variable - `{}`.", var.name);
                    let mut op_range = None;
                    match m {
                        Some(Meta::Multiple) => msg.push_str(&format!(
                            " Use `_` or `_{}` as a variable name to indicate that it won't be used.",
                            var.name
                        )),
                        Some(Meta::Operator(op)) => {
                            op_range = tree.last_child(op).and_then(|v| operator(src, ae, tree.nodes[v].start));
                            let ret = body_node(tree, var.scope)
                                .and_then(|b| if tree.t(b) == T::Begin { tree.last_child(b) } else { Some(b) });
                            if ret == Some(op)
                                && let Some((os, oe)) = op_range
                            {
                                let operator = &src[os..oe];
                                msg.push_str(&format!(
                                    " Use `{}` instead of `{operator}`.",
                                    operator.strip_suffix('=').unwrap_or(operator)
                                ));
                            }
                        }
                        _ => {
                            if scope_names.as_ref().is_none_or(|(k, _)| *k != var.scope.node) {
                                let mut all = Vec::new();
                                if var.scope.naked {
                                    all.push(var.scope.node);
                                }
                                scope_nodes(tree, var.scope, var.scope.node, &mut all);
                                let mut seen = std::collections::HashSet::new();
                                let mut names: Vec<&str> = Vec::new();
                                for n in all {
                                    let pn = &tree.nodes[n];
                                    if pn.t == T::Send
                                        && !pn.csend
                                        && tree.child(n, 0).is_none()
                                        && pn.nargs == 0
                                        && seen.insert(pn.name.as_str())
                                    {
                                        names.push(&pn.name);
                                    }
                                }
                                for &x in &scope.variables {
                                    let n = vars[x].name.as_str();
                                    if seen.insert(n) {
                                        names.push(n);
                                    }
                                }
                                scope_names = Some((var.scope.node, names));
                            }
                            let names: Vec<&str> =
                                scope_names.as_ref().unwrap().1.iter().copied().filter(|n| *n != var.name).collect();
                            if let Some(similar) = helpers::did_you_mean::correct(&var.name, &names).first() {
                                msg.push_str(&format!(" Did you mean `{similar}`?"));
                            }
                        }
                    }
                    let (rs, re) = if tree.t(an) == T::MatchWithLvasgn {
                        match tree.child(an, 0) {
                            Some(k) => tree.range(k),
                            None => continue,
                        }
                    } else {
                        (as_, as_ + var.name.len())
                    };
                    let parent = tree.parent(an);
                    let uncorrectable = sequential_assignment(tree, an)
                        || parent.is_some_and(|p| matches!(tree.t(p), T::OrAsgn | T::AndAsgn));
                    let fix = if uncorrectable {
                        Fix::None
                    } else if let Some(p) = parent.filter(|&p| tree.t(p) == T::Resbody && tree.child(p, 1) == Some(an)) {
                        let from = match tree.child(p, 0) {
                            Some(list) => tree.nodes[list].end,
                            None => tree.nodes[p].start + "rescue".len(),
                        };
                        Fix::Remove(from, ae)
                    } else if matches!(m, Some(Meta::Multiple | Meta::Rest | Meta::For)) {
                        Fix::Replace(as_, ae, "_".into())
                    } else if let Some(Meta::Operator(_)) = m {
                        match op_range {
                            Some((_, oe)) => Fix::Remove(oe - 1, oe),
                            None => Fix::None,
                        }
                    } else if tree.t(an) == T::MatchWithLvasgn {
                        let text = &src[rs..re];
                        Fix::Replace(rs, re, text.replacen(&format!("(?<{}>", var.name), "(?:", 1))
                    } else {
                        match tree.child(an, 0) {
                            // `x = 1, 2` cannot become `1, 2`, so the values become an array literal.
                            Some(expr) if tree.nodes[expr].kind == kind::ARRAY_NODE && !bracketed(node, tree.range(expr)) => {
                                Fix::Bracket(rs, tree.nodes[expr].start, tree.nodes[expr].end)
                            }
                            Some(expr) => Fix::Remove(rs, tree.nodes[expr].start),
                            None => Fix::None,
                        }
                    };
                    offenses.push((rs, re, msg, fix));
                    // chained_assignment?
                    let expr = tree.child(an, 0);
                    if tree.t(an) == T::Lvasgn
                        && expr.is_some_and(|e| (tree.t(e) == T::Send && !tree.nodes[e].csend) || tree.t(e) == T::Lvasgn)
                    {
                        ignored.push((as_, ae));
                    }
                }
            }
        });
        for (s, e, msg, fix) in offenses {
            cx.add_fix(s, e, msg, |c| match &fix {
                Fix::None => {}
                Fix::Bracket(s, v, e) => {
                    c.replace(*s, *v, "[");
                    c.insert_after(*e, *e, "]");
                }
                Fix::Remove(s, e) => c.remove(*s, *e),
                Fix::Replace(s, e, t) => c.replace(*s, *e, t),
            });
        }
    }
}
