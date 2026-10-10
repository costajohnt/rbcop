use crate::prelude::*;
use helpers::parser_tree::{Id, Tree, T};
use helpers::variable_force::VariableForce;

pub const NAME: &str = "Lint/UselessAssignment";
pub static COP: UselessAssignment = UselessAssignment;
pub struct UselessAssignment;

fn is_assignment(t: T) -> bool {
    matches!(t, T::Lvasgn | T::Ivasgn | T::Cvasgn | T::Gvasgn | T::Casgn | T::Masgn | T::OpAsgn | T::OrAsgn | T::AndAsgn)
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
    if matches!(tree.t(parent), T::OpAsgn | T::OrAsgn | T::AndAsgn) && tree.sibling_index(node) == Some(0) {
        return Some(Meta::Operator(parent));
    }
    // multiple_assignment_node
    let mut cand = Some(parent);
    if tree.t(parent) == T::Splat {
        cand = tree.parent(parent);
    }
    if cand.is_some_and(|c| tree.t(c) == T::Mlhs) {
        if let Some(gp) = tree.parent(parent) {
            let mut g = Some(gp);
            while let Some(x) = g.filter(|&x| tree.t(x) == T::Mlhs) {
                let p = tree.parent(x);
                if p.is_some_and(|p| tree.t(p) == T::Masgn) {
                    return Some(Meta::Multiple);
                }
                g = p;
            }
            if tree.t(gp) == T::Masgn {
                return Some(Meta::Multiple);
            }
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

fn sequential_assignment(tree: &Tree, node: Id) -> bool {
    std::iter::once(node).chain(tree.ancestors(node)).any(|n| {
        tree.t(n) == T::Lvasgn
            && tree.kid(n, 1).is_some_and(|e| tree.t(e) == T::Array)
            && tree.descendants(n).iter().any(|&d| is_assignment(tree.t(d)))
    })
}

fn variable_in_loop_condition(tree: &Tree, node: Id, name: &str) -> bool {
    if tree.ancestors(node).any(|a| matches!(tree.t(a), T::Def | T::Defs)) {
        return false;
    }
    let Some(lp) = tree.ancestors(node).find(|&a| matches!(tree.t(a), T::While | T::Until | T::WhilePost | T::UntilPost | T::For))
    else {
        return false;
    };
    if tree.t(lp) == T::For {
        return false;
    }
    let Some(cond) = tree.kid(lp, 0) else { return false };
    std::iter::once(cond).chain(tree.descendants(cond)).any(|n| tree.t(n) == T::Lvar && tree.nodes[n].name == name)
}

enum Fix {
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
        let tree = Tree::build(node);
        let vf = VariableForce::run(&tree);
        let src = cx.src.text;
        let mut ignored: Vec<(usize, usize)> = Vec::new();
        for &s in &vf.left {
            for &v in &vf.scopes[s].vars {
                let var = &vf.vars[v];
                if var.name.starts_with('_') {
                    continue;
                }
                for &a in var.assignments.iter().rev() {
                    let an = vf.assignments[a].node;
                    let (as_, ae) = tree.range(an);
                    if vf.assignment_used(a)
                        || ignored.iter().any(|&(s, e)| s <= as_ && e >= ae)
                        || variable_in_loop_condition(&tree, an, &var.name)
                    {
                        continue;
                    }
                    let m = meta(&tree, an);
                    let mut msg = format!("Useless assignment to variable - `{}`.", var.name);
                    match m {
                        Some(Meta::Multiple) => msg.push_str(&format!(
                            " Use `_` or `_{}` as a variable name to indicate that it won't be used.",
                            var.name
                        )),
                        Some(Meta::Operator(op)) => {
                            let body = vf.body_node(var.scope);
                            let ret = body.and_then(|b| if tree.t(b) == T::Begin { tree.nodes[b].kids.last().copied().flatten() } else { Some(b) });
                            if ret == Some(op) {
                                let (os, oe) = tree.nodes[op].op_loc.unwrap_or((0, 0));
                                let operator = &src[os..oe];
                                msg.push_str(&format!(
                                    " Use `{}` instead of `{}`.",
                                    operator.strip_suffix('=').unwrap_or(operator),
                                    operator
                                ));
                            }
                        }
                        _ => {
                            let mut names: Vec<&str> = Vec::new();
                            for n in vf.scope_nodes(var.scope) {
                                let pn = &tree.nodes[n];
                                if pn.t == T::Send && pn.kids.len() == 2 && pn.kids[0].is_none() && !names.contains(&pn.name.as_str()) {
                                    names.push(&pn.name);
                                }
                            }
                            for &x in &vf.scopes[var.scope].vars {
                                let n = vf.vars[x].name.as_str();
                                if !names.contains(&n) {
                                    names.push(n);
                                }
                            }
                            names.retain(|n| *n != var.name);
                            if let Some(similar) = helpers::did_you_mean::correct(&var.name, &names).first() {
                                msg.push_str(&format!(" Did you mean `{similar}`?"));
                            }
                        }
                    }
                    let range = if tree.t(an) == T::MatchWithLvasgn {
                        tree.kid(an, 0).map(|k| tree.range(k))
                    } else {
                        tree.nodes[an].name_loc
                    };
                    let Some((rs, re)) = range else { continue };
                    let parent = tree.parent(an);
                    let uncorrectable = sequential_assignment(&tree, an)
                        || parent.is_some_and(|p| matches!(tree.t(p), T::OrAsgn | T::AndAsgn));
                    let fix = if uncorrectable {
                        Fix::None
                    } else if let Some(p) = parent.filter(|&p| tree.t(p) == T::Resbody && tree.kid(p, 1) == Some(an)) {
                        let from = match tree.kid(p, 0) {
                            Some(list) => tree.nodes[list].end,
                            None => tree.nodes[p].op_loc.unwrap().1,
                        };
                        Fix::Remove(from, ae)
                    } else if matches!(m, Some(Meta::Multiple | Meta::Rest | Meta::For)) {
                        Fix::Replace(as_, ae, "_".into())
                    } else if let Some(Meta::Operator(op)) = m {
                        match tree.nodes[op].op_loc {
                            Some((_, oe)) => Fix::Remove(oe - 1, oe),
                            None => Fix::None,
                        }
                    } else if tree.t(an) == T::MatchWithLvasgn {
                        let re_node = tree.kid(an, 0).unwrap();
                        let (s, e) = tree.range(re_node);
                        let text = &src[s..e];
                        let needle = format!("(?<{}>", var.name);
                        Fix::Replace(s, e, text.replacen(&needle, "(?:", 1))
                    } else {
                        match tree.kid(an, 1) {
                            Some(expr) => Fix::Remove(rs, tree.nodes[expr].start),
                            None => Fix::None,
                        }
                    };
                    cx.add_fix(rs, re, msg, |c| match fix {
                        Fix::None => {}
                        Fix::Remove(s, e) => c.remove(s, e),
                        Fix::Replace(s, e, t) => c.replace(s, e, &t),
                    });
                    // chained_assignment?
                    let expr = tree.kid(an, 1);
                    if tree.t(an) == T::Lvasgn && expr.is_some_and(|e| matches!(tree.t(e), T::Send | T::Lvasgn)) {
                        ignored.push((as_, ae));
                    }
                }
            }
        }
    }
}
