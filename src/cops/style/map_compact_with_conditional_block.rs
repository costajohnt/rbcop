use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Style/MapCompactWithConditionalBlock";
pub static COP: MapCompactWithConditionalBlock = MapCompactWithConditionalBlock;
pub struct MapCompactWithConditionalBlock;

/// A parser branch: None when empty, Some(node) for a single statement; 2+ statements never match.
fn branch<'pr>(s: Option<ruby_prism::StatementsNode<'pr>>) -> Result<Option<Node<'pr>>, ()> {
    let Some(s) = s else { return Ok(None) };
    let v: Vec<Node> = s.body().iter().collect();
    match v.len() {
        0 => Ok(None),
        1 => Ok(v.into_iter().next()),
        _ => Err(()),
    }
}

/// Parser `(if cond then else)` children of an if/unless/ternary (unless swapped).
fn if_children<'pr>(n: &Node<'pr>) -> Option<(Node<'pr>, Option<Node<'pr>>, Option<Node<'pr>>)> {
    if let Some(i) = n.as_if_node() {
        let els = match i.subsequent() {
            None => None,
            Some(s) => match s.as_else_node() {
                Some(e) => branch(e.statements()).ok()?,
                None => Some(s),
            },
        };
        return Some((i.predicate(), branch(i.statements()).ok()?, els));
    }
    let u = n.as_unless_node()?;
    let els = match u.else_clause() {
        None => None,
        Some(e) => branch(e.statements()).ok()?,
    };
    Some((u.predicate(), els, branch(u.statements()).ok()?))
}

fn is_lvar(n: &Option<Node>) -> bool {
    n.as_ref().is_some_and(|n| n.as_local_variable_read_node().is_some())
}

/// NodePattern `next`: any `next` node, with or without a value.
fn any_next(n: &Option<Node>) -> bool {
    n.as_ref().is_some_and(|n| n.as_next_node().is_some())
}

/// `(next $(lvar _))`: the lvar.
fn next_lvar<'pr>(n: &Option<Node<'pr>>) -> Option<Node<'pr>> {
    let args = n.as_ref()?.as_next_node()?.arguments()?;
    let v: Vec<Node> = args.arguments().iter().collect();
    let [a] = v.as_slice() else { return None };
    a.as_local_variable_read_node()?;
    v.into_iter().next()
}

fn nil_lit(n: &Option<Node>) -> bool {
    n.as_ref().is_some_and(|n| n.as_nil_node().is_some())
}

struct Match<'pr> {
    arg: Node<'pr>,
    cond: Node<'pr>,
    ret: Node<'pr>,
    /// truthy_branch?
    select: bool,
}

/// `conditional_block`: the call owns a `{ |e| ... }` block of the expected shape.
fn conditional_block<'pr>(call: &CallNode<'pr>) -> Option<Match<'pr>> {
    if !matches!(method_name(call), "map" | "filter_map") || call.arguments().is_some() {
        return None;
    }
    let block = call.block()?.as_block_node()?;
    let params = block.parameters()?.as_block_parameters_node()?;
    let mut args = helpers::block_args::children(&params);
    if args.len() != 1 || args[0].as_required_parameter_node().is_none() {
        return None;
    }
    let arg = args.pop().unwrap();
    let stmts: Vec<Node> = block.body()?.as_statements_node()?.body().iter().collect();
    match stmts.len() {
        1 => {
            let body = stmts.into_iter().next().unwrap();
            let (cond, t, e) = if_children(&body)?;
            // truthy_branch_for_if?: `if`/ternary want the if_branch, `unless` the else_branch; both
            // are the parser's second child.
            let truthy = |ret_in_then: bool| ret_in_then;
            let next_or_nil = |b: &Option<Node>| any_next(b) || b.is_none();
            let next_nil_or_nil = |b: &Option<Node>| any_next(b) || nil_lit(b) || b.is_none();
            if is_lvar(&t) && next_or_nil(&e) {
                return Some(Match { arg, cond, ret: t.unwrap(), select: truthy(true) });
            }
            if next_or_nil(&t) && is_lvar(&e) {
                return Some(Match { arg, cond, ret: e.unwrap(), select: truthy(false) });
            }
            if let Some(ret) = next_lvar(&t).filter(|_| next_nil_or_nil(&e)) {
                return Some(Match { arg, cond, ret, select: truthy(true) });
            }
            if let Some(ret) = next_lvar(&e).filter(|_| next_nil_or_nil(&t)) {
                return Some(Match { arg, cond, ret, select: truthy(false) });
            }
            None
        }
        2 => {
            let mut it = stmts.into_iter();
            let (guard, last) = (it.next().unwrap(), it.next().unwrap());
            let (cond, t, e) = if_children(&guard)?;
            let is_if = guard.as_if_node().is_some_and(|i| i.if_keyword_loc().is_some_and(|k| k.as_slice() == b"if"));
            if last.as_local_variable_read_node().is_some() {
                if (any_next(&t) && e.is_none()) || (t.is_none() && any_next(&e)) {
                    // truthy_branch_for_guard?: whether the `next` in if_branch has a value; a
                    // missing if_branch raises in RuboCop.
                    let if_branch = if guard.as_unless_node().is_some() { &e } else { &t };
                    let has_value = if_branch.as_ref()?.as_next_node().is_some_and(|x| x.arguments().is_some());
                    return Some(Match { arg, cond, ret: last, select: if is_if { has_value } else { !has_value } });
                }
                return None;
            }
            last.as_nil_node()?;
            let (ret, ret_in_then) = match (next_lvar(&t), next_lvar(&e)) {
                (Some(r), _) if e.is_none() => (r, true),
                (_, Some(r)) if t.is_none() => (r, false),
                _ => return None,
            };
            Some(Match { arg, cond, ret, select: ret_in_then })
        }
        _ => None,
    }
}

impl Cop for MapCompactWithConditionalBlock {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "compact" | "filter_map") {
            return;
        }
        let (m, (s, e), current) = if let Some(map) = call.receiver().and_then(|r| r.as_call_node()).filter(|m| conditional_block(m).is_some()) {
            if name != "compact" || call.arguments().is_some() || call.block().is_some_and(|b| b.as_block_argument_node().is_some()) {
                return;
            }
            let Some(sel) = map.message_loc() else { return };
            let current = format!("{} {{ ... }}.compact", method_name(&map));
            (conditional_block(&map).unwrap(), (sel.start_offset(), send_range(&call).1), current)
        } else if let Some(m) = conditional_block(&call) {
            if name != "filter_map" {
                return;
            }
            let Some(sel) = call.message_loc() else { return };
            (m, (sel.start_offset(), node.location().end_offset()), "filter_map { ... }".to_string())
        } else {
            return;
        };
        let arg_name = m.arg.as_required_parameter_node().unwrap().name();
        if arg_name.as_slice() != m.ret.as_local_variable_read_node().unwrap().name().as_slice() {
            return;
        }
        let method = if m.select { "select" } else { "reject" };
        let node_r = send_range(&call);
        let ignored = cx.state::<Vec<(usize, usize)>>();
        if ignored.iter().any(|&(a, b)| a <= node_r.0 && node_r.1 <= b) {
            // `return` inside the corrector block drops the offense.
            return;
        }
        let msg = format!("Replace `{current}` with `{method}`.");
        let repl = format!("{method} {{ |{}| {} }}", node_text(&m.arg), node_text(&m.cond));
        let mut ran = false;
        cx.add_fix(s, e, msg, |c| {
            c.replace(s, e, &repl);
            ran = true;
        });
        if ran {
            cx.state::<Vec<(usize, usize)>>().push(node_r);
        }
    }
}
