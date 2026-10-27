use crate::prelude::*;

pub const NAME: &str = "Style/TrailingUnderscoreVariable";
pub static COP: TrailingUnderscoreVariable = TrailingUnderscoreVariable;
pub struct TrailingUnderscoreVariable;

/// A parser mlhs: its variables, parens and range.
struct Mlhs<'pr> {
    vars: Vec<Node<'pr>>,
    parens: bool,
    start: usize,
    end: usize,
}

fn mlhs<'pr>(
    lefts: ruby_prism::NodeList<'pr>,
    rest: Option<Node<'pr>>,
    rights: ruby_prism::NodeList<'pr>,
    parens: Option<(usize, usize)>,
) -> Mlhs<'pr> {
    let vars: Vec<Node> = lefts.iter().chain(parser_rest(rest)).chain(rights.iter()).collect();
    let (start, end) = match parens {
        Some(p) => p,
        None => (
            vars.first().map_or(0, |v| v.location().start_offset()),
            vars.last().map_or(0, |v| v.location().end_offset()),
        ),
    };
    Mlhs { vars, parens: parens.is_some(), start, end }
}

fn target_mlhs<'pr>(node: &Node<'pr>) -> Option<Mlhs<'pr>> {
    let t = node.as_multi_target_node()?;
    let parens = t.lparen_loc().zip(t.rparen_loc()).map(|(l, r)| (l.start_offset(), r.end_offset()));
    Some(mlhs(t.lefts(), t.rest(), t.rights(), parens))
}

/// `var, = *variable; var, = *var` for an lvasgn or splat: the variable name (None for `*`).
/// None as the outer value when the variable is neither (DISALLOW).
fn var_name<'pr>(node: &Node<'pr>) -> Option<Option<&'pr [u8]>> {
    if let Some(l) = node.as_local_variable_target_node() {
        return Some(Some(l.name().as_slice()));
    }
    let s = node.as_splat_node()?;
    Some(match s.expression() {
        None => None,
        Some(e) => match e.as_local_variable_target_node() {
            Some(l) => Some(l.name().as_slice()),
            // `*@a` / `*a.b`: `*var` on a non-lvasgn child; its first child is never `_`-named
            None => Some(b"".as_slice()),
        },
    })
}

fn find_first_possible_offense(vars: &[Node], allow_named: bool) -> Option<usize> {
    let mut offense = None;
    for (i, v) in vars.iter().enumerate().rev() {
        let Some(name) = var_name(v) else { break };
        let name = name.unwrap_or(b"");
        if (allow_named && name != b"_") || !name.starts_with(b"_") {
            break;
        }
        offense = Some(i);
    }
    offense
}

fn find_first_offense<'pr>(vars: &[Node<'pr>], allow_named: bool) -> Option<usize> {
    let first = find_first_possible_offense(vars, allow_named)?;
    // reverse_index compares structurally: the last variable equal to the offense.
    let key = |n: &Node<'pr>| (n.as_splat_node().is_some(), var_name(n));
    let idx = (0..vars.len()).rev().find(|&i| key(&vars[i]) == key(&vars[first])).unwrap_or(first);
    if vars[..idx].iter().any(|v| v.as_splat_node().is_some()) {
        return None;
    }
    Some(first)
}

/// The right-hand side of a masgn: its start, the operator start, and whether it is a list of
/// values without brackets (`_, _ = 1, 2`).
type Masgn = (usize, usize, bool);

/// unneeded_ranges for a masgn or a nested mlhs: each range with its replacement.
fn unneeded_ranges(m: &Mlhs, masgn: Option<Masgn>, allow_named: bool, out: &mut Vec<((usize, usize), &'static str)>) {
    for v in &m.vars {
        if let Some(child) = target_mlhs(v) {
            unneeded_ranges(&child, None, allow_named, out);
        }
    }
    let Some(first) = find_first_offense(&m.vars, allow_named) else { return };
    let offense_start = m.vars[first].location().start_offset();
    let range = if first == 0 {
        // unused_variables_only?: a nested group collapses into a single `_` (the parent group
        // handles it in turn); `_, _ = 1, 2` keeps its values as an array literal.
        match masgn {
            None => ((m.start, m.end), "_"),
            Some((rhs, _, bare_list)) => ((m.start, rhs), if bare_list { "[" } else { "" }),
        }
    } else if m.parens {
        ((offense_start - 1, m.end - 1), "")
    } else {
        ((offense_start, masgn.map_or(m.end, |(_, op, _)| op)), "")
    };
    out.push(range);
}

impl Cop for TrailingUnderscoreVariable {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::MULTI_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let w = node.as_multi_write_node().unwrap();
        let allow_named = cx.cfg.bool("AllowNamedUnderscoreVariables").unwrap_or(true);
        let parens = w.lparen_loc().zip(w.rparen_loc()).map(|(l, r)| (l.start_offset(), r.end_offset()));
        let m = mlhs(w.lefts(), w.rest(), w.rights(), parens);
        let value = w.value();
        let bare_list = value.as_array_node().is_some_and(|a| a.opening_loc().is_none());
        let masgn = (value.location().start_offset(), w.operator_loc().start_offset(), bare_list);
        let mut ranges = Vec::new();
        unneeded_ranges(&m, Some(masgn), allow_named, &mut ranges);
        let loc = node.location();
        let (ns, ne) = (loc.start_offset(), loc.end_offset());
        let src = cx.src.slice(ns, ne);
        for ((start, end), replacement) in ranges {
            let closing = if replacement == "[" { "]" } else { "" };
            // good_code: the masgn source with the range replaced.
            let good = format!("{}{replacement}{}{closing}", &src[..start - ns], &src[end - ns..]);
            let msg = format!("Do not use trailing `_`s in parallel assignment. Prefer `{good}`.");
            cx.add_fix(start, end, msg, |c| {
                c.replace(start, end, replacement);
                if !closing.is_empty() {
                    c.insert_after(ns, ne, closing);
                }
            });
        }
    }
}
