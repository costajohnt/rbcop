use crate::prelude::*;
use helpers::node_equality::same;
use helpers::parser_parent::parser_parent_index;
use helpers::whole_lines::range_by_whole_lines;
use crate::source::Source;

pub const NAME: &str = "Style/PartitionInsteadOfDoubleSelect";
pub static COP: PartitionInsteadOfDoubleSelect = PartitionInsteadOfDoubleSelect;
pub struct PartitionInsteadOfDoubleSelect;

const SELECT: &[&str] = &["select", "filter", "find_all"];

fn range(n: &Node) -> (usize, usize) {
    let l = n.location();
    (l.start_offset(), l.end_offset())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockType {
    Block,
    Numblock,
    Itblock,
}

/// A parser `block`/`numblock`/`itblock` or a send whose last argument is a block_pass.
struct Cand<'pr> {
    range: (usize, usize),
    name: &'pr str,
    receiver: Option<(usize, usize)>,
    /// (selector start, selector end)
    selector: Option<(usize, usize)>,
    /// None for a send with a block_pass
    block: Option<BlockType>,
    /// block arguments source range (None: no arguments)
    params: Option<(usize, usize)>,
    /// single-argument name of a plain block
    single_arg: Option<&'pr [u8]>,
    body: Option<Node<'pr>>,
    /// the block_pass child (`&:sym`), for sends
    block_pass: Option<Node<'pr>>,
    block_pass_range: Option<(usize, usize)>,
}

fn block_type(params: Option<&Node>) -> BlockType {
    match params {
        Some(p) if p.as_numbered_parameters_node().is_some() => BlockType::Numblock,
        Some(p) if p.as_it_parameters_node().is_some() => BlockType::Itblock,
        _ => BlockType::Block,
    }
}

/// Parser `args` of a plain block: the parameter list's range (None for none or `||`), and the
/// name of a single plain argument.
fn block_params<'pr>(params: Option<Node<'pr>>) -> (Option<(usize, usize)>, Option<&'pr [u8]>) {
    let Some(bp) = params.as_ref().and_then(|p| p.as_block_parameters_node()) else { return (None, None) };
    let empty_locals = bp.locals().iter().next().is_none();
    let Some(p) = bp.parameters() else {
        return (if empty_locals { None } else { Some(range(&params.unwrap())) }, None);
    };
    let only_one = p.optionals().iter().next().is_none()
        && p.rest().is_none()
        && p.posts().iter().next().is_none()
        && p.keywords().iter().next().is_none()
        && p.keyword_rest().is_none()
        && p.block().is_none()
        && empty_locals
        && p.requireds().iter().count() == 1;
    let single = if only_one {
        p.requireds().iter().next().and_then(|r| r.as_required_parameter_node()).map(|r| r.name().as_slice())
    } else {
        None
    };
    // `|x|` and `(x)` hold the same parser args: compare the parameters themselves.
    (Some(range(&p.as_node())), single)
}

fn cand<'pr>(n: &Node<'pr>) -> Option<Cand<'pr>> {
    if let Some(l) = n.as_lambda_node() {
        let (params, single_arg) = block_params(l.parameters());
        return Some(Cand {
            range: range(n),
            name: "lambda",
            receiver: None,
            selector: None,
            block: Some(block_type(l.parameters().as_ref())),
            params,
            single_arg,
            body: l.body(),
            block_pass: None,
            block_pass_range: None,
        });
    }
    let call = n.as_call_node()?;
    let sel = call.message_loc().map(|s| (s.start_offset(), s.end_offset()));
    let receiver = call.receiver().map(|r| range(&r));
    match call.block() {
        Some(b) if b.as_block_node().is_some() => {
            let b = b.as_block_node().unwrap();
            let (params, single_arg) = block_params(b.parameters());
            Some(Cand {
                range: range(n),
                name: method_name(&call),
                receiver,
                selector: sel,
                block: Some(block_type(b.parameters().as_ref())),
                params,
                single_arg,
                body: b.body(),
                block_pass: None,
                block_pass_range: None,
            })
        }
        Some(b) => {
            let ba = b.as_block_argument_node()?;
            Some(Cand {
                range: send_range(&call),
                name: method_name(&call),
                receiver,
                selector: sel,
                block: None,
                params: None,
                single_arg: None,
                body: None,
                block_pass: ba.expression(),
                block_pass_range: Some(range(&b)),
            })
        }
        None => None,
    }
}

fn body_range_of(body: &Option<Node>) -> Option<(usize, usize)> {
    body.as_ref().map(body_range)
}

fn opt_same(src: &Source, a: Option<(usize, usize)>, b: Option<(usize, usize)>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same(src, a, b),
        _ => false,
    }
}

/// The single statement of a block body (parser body when it is not a `begin`).
fn single_statement<'pr>(body: &Option<Node<'pr>>) -> Option<Node<'pr>> {
    let s = body.as_ref()?.as_statements_node()?;
    let mut it = s.body().iter();
    match (it.next(), it.next()) {
        (Some(n), None) => Some(n),
        _ => None,
    }
}

fn is_select(name: &str) -> bool {
    SELECT.contains(&name)
}

fn complementary(a: &str, b: &str) -> bool {
    (is_select(a) && b == "reject") || (a == "reject" && is_select(b))
}

/// `(block _ (args (arg _name)) (send (lvar _name) $_method_name))`
fn symbol_proc_method<'pr>(c: &Cand<'pr>) -> Option<&'pr [u8]> {
    if c.block != Some(BlockType::Block) {
        return None;
    }
    let arg = c.single_arg?;
    let send = single_statement(&c.body)?.as_call_node()?;
    if is_csend(&send) || send.block().is_some() || send.arguments().is_some() {
        return None;
    }
    let recv = send.receiver()?;
    (recv.as_local_variable_read_node()?.name().as_slice() == arg).then(|| send.name().as_slice())
}

fn block_matches_block_pass(block: &Cand, send: &Cand) -> bool {
    let Some(m) = symbol_proc_method(block) else { return false };
    send.block_pass.as_ref().and_then(|s| s.as_symbol_node()).is_some_and(|s| s.unescaped() == m)
}

fn equivalent_predicate(src: &Source, a: &Cand, b: &Cand) -> bool {
    match (a.block, b.block) {
        (Some(ta), Some(tb)) => {
            ta == tb
                && (ta != BlockType::Block || opt_same(src, a.params, b.params))
                && opt_same(src, body_range_of(&a.body), body_range_of(&b.body))
        }
        (Some(_), None) => block_matches_block_pass(a, b),
        (None, Some(_)) => block_matches_block_pass(b, a),
        (None, None) => opt_same(src, a.block_pass_range, b.block_pass_range),
    }
}

/// `body1&.send_type? && body1.method?(:!) && body1.receiver == body2`
fn negated_body(src: &Source, b1: &Option<Node>, b2: &Option<Node>) -> bool {
    let Some(s) = single_statement(b1).and_then(|n| n.as_call_node()) else { return false };
    if is_csend(&s) || method_name(&s) != "!" || s.block().is_some_and(|b| b.as_block_node().is_some()) {
        return false;
    }
    let Some(r) = s.receiver() else { return false };
    opt_same(src, Some(range(&r)), body_range_of(b2))
}

fn negated_predicate(src: &Source, a: &Cand, b: &Cand) -> bool {
    let (Some(ta), Some(tb)) = (a.block, b.block) else { return false };
    if ta != tb || (ta == BlockType::Block && !opt_same(src, a.params, b.params)) {
        return false;
    }
    negated_body(src, &a.body, &b.body) || negated_body(src, &b.body, &a.body)
}

/// Parser `begin` parent: a statement list of 2+ statements, except directly in a kwbegin.
fn begin_statements<'pr>(anc: &[Node<'pr>], i: usize) -> Option<Vec<Node<'pr>>> {
    let s = anc[i].as_statements_node()?;
    let stmts: Vec<Node> = s.body().iter().collect();
    if stmts.len() < 2 {
        return None;
    }
    let kwbegin = i > 0
        && anc[i - 1].as_begin_node().is_some_and(|b| {
            b.begin_keyword_loc().is_some()
                && b.rescue_clause().is_none()
                && b.ensure_clause().is_none()
                && b.statements().is_some_and(|st| range(&st.as_node()) == range(&anc[i]))
        });
    (!kwbegin).then_some(stmts)
}

/// `children.last` of an assignment (value), setter sends included.
fn assigned_value<'pr>(n: &Node<'pr>) -> Option<Node<'pr>> {
    if let Some(c) = n.as_call_node() {
        return c.equal_loc().and_then(|_| send_args(&c).pop());
    }
    assignment_parts(n).map(|(_, v)| v)
}

fn lvasgn_name<'pr>(n: &Node<'pr>) -> Option<&'pr str> {
    n.as_local_variable_write_node().map(|w| std::str::from_utf8(w.name().as_slice()).unwrap_or(""))
}

fn build_partition_call(src: &Source, c: &Cand) -> Option<String> {
    let (s, e) = c.selector?;
    let (start, end) = c.range;
    Some(format!("{}partition{}", &src.text[start..s], &src.text[e..end]))
}

impl Cop for PartitionInsteadOfDoubleSelect {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !(is_select(name) || name == "reject") {
            return;
        }
        let Some(me) = cand(node) else { return };
        let anc = cx.ancestors;
        let src = cx.src;
        // node_container
        let Some(pi) = parser_parent_index(anc, anc.len()) else { return };
        // (container range, its lvasgn name, the begin's statements)
        let (crange, var, statements) = if let Some(stmts) = begin_statements(anc, pi) {
            (range(node), None, stmts)
        } else if is_assignment(&anc[pi]) {
            let Some(ppi) = parser_parent_index(anc, pi) else { return };
            let Some(stmts) = begin_statements(anc, ppi) else { return };
            (range(&anc[pi]), lvasgn_name(&anc[pi]), stmts)
        } else {
            return;
        };
        let Some(idx) = statements.iter().position(|s| range(s) == crange) else { return };
        if idx == 0 {
            return;
        }
        let sibling_container = &statements[idx - 1];
        // extract_candidate
        let sibling = if cand(sibling_container).is_some_and(|c| c.block.is_some()) {
            cand(sibling_container)
        } else if is_assignment(sibling_container) {
            assigned_value(sibling_container).and_then(|v| cand(&v))
        } else {
            cand(sibling_container)
        };
        let Some(sibling) = sibling else { return };
        if !opt_same(src, me.receiver, sibling.receiver) {
            return;
        }
        let complementary_pair = complementary(me.name, sibling.name);
        let matching = (complementary_pair && equivalent_predicate(src, &me, &sibling))
            || (me.name == sibling.name && negated_predicate(src, &me, &sibling));
        if !matching {
            return;
        }
        let msg = format!("Use `partition` instead of consecutive `{}` and `{}` calls.", sibling.name, me.name);
        // autocorrect (both containers lvasgn)
        let fix = match (var, lvasgn_name(sibling_container)) {
            (Some(var), Some(sib_var)) => {
                let (select_var, reject_var, partition) = if complementary_pair {
                    let vars = if is_select(sibling.name) { (sib_var, var) } else { (var, sib_var) };
                    (vars.0, vars.1, if is_select(sibling.name) { &sibling } else { &me })
                } else {
                    let negated = negated_body(src, &me.body, &sibling.body);
                    let truthy = is_select(me.name) != negated;
                    let p = if negated { &sibling } else { &me };
                    if truthy { (var, sib_var, p) } else { (sib_var, var, p) }
                };
                build_partition_call(src, partition).map(|call| {
                    let replacement = format!("{select_var}, {reject_var} = {call}");
                    let lines = range_by_whole_lines(src.text, crange.0, crange.1, true);
                    (range(sibling_container), replacement, lines)
                })
            }
            _ => None,
        };
        cx.add_fix(crange.0, crange.1, msg, |c| {
            if let Some((sib, replacement, lines)) = &fix {
                c.replace(sib.0, sib.1, replacement);
                c.remove(lines.0, lines.1);
            }
        });
    }
}
