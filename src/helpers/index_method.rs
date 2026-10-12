//! rubocop-rails IndexMethod (Rails/IndexBy, Rails/IndexWith): `each_with_object`, `to_h { }`,
//! `map { }.to_h` and `Hash[map { }]` that build a hash keyed (index_by) or valued (index_with) by
//! the element.

use crate::prelude::*;
use ruby_prism::{BlockNode, CallNode, Visit};

/// Which element of the pair the element itself must be: `index_by` builds `[key, el]`,
/// `index_with` builds `[el, value]`.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    By,
    With,
}

/// The block parameter: a plain `|el|` / `|el, memo|`, a numbered `_1` block, or an `it` block.
enum Param {
    Named(String),
    Numbered,
    It,
}

/// Block parameters as parser args: Some(names) for plain required args only.
fn plain_args(b: &BlockNode) -> Option<Vec<String>> {
    let p = b.parameters()?;
    let bp = p.as_block_parameters_node()?;
    if bp.locals().iter().next().is_some() {
        return None;
    }
    let params = bp.parameters()?;
    if params.optionals().iter().next().is_some()
        || params.rest().is_some()
        || params.posts().iter().next().is_some()
        || params.keywords().iter().next().is_some()
        || params.keyword_rest().is_some()
        || params.block().is_some()
    {
        return None;
    }
    params
        .requireds()
        .iter()
        .map(|r| r.as_required_parameter_node().map(|n| String::from_utf8_lossy(n.name().as_slice()).into_owned()))
        .collect()
}

fn param(b: &BlockNode) -> Option<Param> {
    match b.parameters() {
        Some(p) if p.as_numbered_parameters_node().is_some_and(|n| n.maximum() == 1) => Some(Param::Numbered),
        Some(p) if p.as_numbered_parameters_node().is_some() => None,
        Some(p) if p.as_it_parameters_node().is_some() => Some(Param::It),
        _ => match plain_args(b)?.as_slice() {
            [one] => Some(Param::Named(one.clone())),
            _ => None,
        },
    }
}

/// `(lvar <name>)` for the block's element parameter.
fn is_el(n: &Node, p: &Param) -> bool {
    match p {
        Param::Named(name) => n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name.as_bytes()),
        Param::Numbered => n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == b"_1"),
        Param::It => n.as_it_local_variable_read_node().is_some(),
    }
}

/// The single body expression of a block (parser: no `begin`).
fn single_body<'pr>(b: &BlockNode<'pr>) -> Option<Node<'pr>> {
    let body = b.body()?;
    let s = body.as_statements_node()?;
    let mut it = s.body().iter();
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

/// NodePattern `` `_memo ``: any descendant (or symbol child) named `memo`.
fn mentions(n: &Node, memo: &str) -> bool {
    struct F<'a>(&'a [u8], bool);
    impl<'pr> Visit<'pr> for F<'_> {
        fn visit_local_variable_read_node(&mut self, n: &ruby_prism::LocalVariableReadNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
        }
        fn visit_local_variable_write_node(&mut self, n: &ruby_prism::LocalVariableWriteNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
            ruby_prism::visit_local_variable_write_node(self, n);
        }
        fn visit_local_variable_target_node(&mut self, n: &ruby_prism::LocalVariableTargetNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
        }
        fn visit_call_node(&mut self, n: &CallNode<'pr>) {
            self.1 |= n.name().as_slice() == self.0;
            ruby_prism::visit_call_node(self, n);
        }
        fn visit_symbol_node(&mut self, n: &ruby_prism::SymbolNode<'pr>) {
            self.1 |= n.unescaped() == self.0;
        }
    }
    let mut f = F(memo.as_bytes(), false);
    f.visit(n);
    f.1
}

/// A block's (Prism call, block) when the call owns a literal block.
fn block_of<'pr>(n: &Node<'pr>) -> Option<(CallNode<'pr>, BlockNode<'pr>)> {
    let c = n.as_call_node()?;
    let b = c.block()?.as_block_node()?;
    Some((c, b))
}

/// `(array $_ (lvar _el))` (By) or `(array (lvar _el) $_)` (With): the other element.
fn pair_body<'pr>(body: &Node<'pr>, p: &Param, kind: Kind) -> Option<Node<'pr>> {
    let a = body.as_array_node()?;
    if a.opening_loc().is_none_or(|o| o.as_slice() != b"[") {
        return None;
    }
    let mut els: Vec<Node> = a.elements().iter().collect();
    if els.len() != 2 {
        return None;
    }
    let (el, other) = match kind {
        Kind::By => (1, 0),
        Kind::With => (0, 1),
    };
    if !is_el(&els[el], p) {
        return None;
    }
    Some(els.swap_remove(other))
}

/// A match: the block node's call, the block, the parameter name (`argname`) and the
/// transforming body expression.
struct Match<'pr> {
    call: CallNode<'pr>,
    block: BlockNode<'pr>,
    argname: Option<String>,
    body: Node<'pr>,
}

/// on_bad_each_with_object on a block (`node` is the call owning it).
fn each_with_object<'pr>(node: &Node<'pr>, kind: Kind) -> Option<Match<'pr>> {
    let (call, block) = block_of(node)?;
    let args = send_args(&call);
    // (call _ :each_with_object (hash))
    if method_name(&call) != "each_with_object"
        || !matches!(args.as_slice(), [h] if h.as_hash_node().is_some_and(|h| h.elements().iter().next().is_none()))
    {
        return None;
    }
    let names = plain_args(&block)?;
    let [el, memo] = names.as_slice() else { return None };
    let body = single_body(&block)?;
    let asgn = body.as_call_node()?;
    if method_name(&asgn) != "[]=" || !asgn.receiver().is_some_and(|r| is_el(&r, &Param::Named(memo.clone()))) {
        return None;
    }
    let mut a = send_args(&asgn);
    if a.len() != 2 {
        return None;
    }
    let p = Param::Named(el.clone());
    let (el_i, other) = match kind {
        Kind::By => (1, 0),
        Kind::With => (0, 1),
    };
    if !is_el(&a[el_i], &p) || mentions(&a[other], memo) {
        return None;
    }
    let body = a.swap_remove(other);
    Some(Match { call, block, argname: Some(el.clone()), body })
}

/// on_bad_to_h: `(block (call _ :to_h) (args (arg $_el)) (array ...))` and the numbered/`it` forms;
/// with `methods` {map collect} it is the inner block of on_bad_map_to_h / hash brackets.
fn array_block<'pr>(node: &Node<'pr>, methods: &[&str], kind: Kind) -> Option<Match<'pr>> {
    let (call, block) = block_of(node)?;
    if !methods.contains(&method_name(&call)) || !send_args(&call).is_empty() {
        return None;
    }
    let p = param(&block)?;
    let body = pair_body(&single_body(&block)?, &p, kind)?;
    let argname = match &p {
        Param::Named(n) => Some(n.clone()),
        _ => None,
    };
    Some(Match { call, block, argname, body })
}

/// Captures#noop_transformation?: the body is just the element.
fn noop(m: &Match) -> bool {
    m.argname.as_ref().is_some_and(|a| m.body.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == a.as_bytes()))
}

/// Ranges an earlier offense of this cop ignored (`ignore_node`); a nested offense's corrector
/// then does nothing.
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

fn register(cx: &mut Cx, node_range: (usize, usize), m: &Match, desc: &str, new_name: &str, leading: usize, trailing: usize) {
    if noop(m) {
        return;
    }
    let (s, e) = node_range;
    let ignored = cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= s && e <= b);
    let msg = format!("Prefer `{new_name}` over `{desc}`.");
    // set_new_method_name: selector, through the closing parenthesis when there are arguments.
    let sel = m.call.message_loc().unwrap();
    let name_end = m.call.closing_loc().map_or(sel.end_offset(), |c| c.end_offset());
    let name_range = (sel.start_offset(), name_end);
    // set_new_arg_name: only for a plain block (`|el, memo|` -> `|el|`).
    let args_range = m.block.parameters().filter(|p| p.as_block_parameters_node().is_some()).map(|p| {
        (p.location().start_offset(), p.location().end_offset())
    });
    let argname = m.argname.clone();
    let body_src = node_text(&m.body);
    let body_src = if m.body.as_keyword_hash_node().is_some() { format!("{{ {body_src} }}") } else { body_src.to_string() };
    let block_body = m.block.body().map(|b| (b.location().start_offset(), b.location().end_offset()));
    cx.add_fix(s, e, msg, |c| {
        if ignored {
            return;
        }
        c.remove_leading(s, e, leading);
        c.remove_trailing(s, e, trailing);
        c.replace(name_range.0, name_range.1, new_name);
        if let (Some((a, b)), Some(name)) = (args_range, &argname) {
            c.replace(a, b, &format!("|{name}|"));
        }
        if let Some((a, b)) = block_body {
            c.replace(a, b, &body_src);
        }
    });
    cx.state::<Ignored>().0.push((s, e));
}

/// IndexMethod#on_block (visited as the call owning the block) and #on_send / #on_csend.
pub fn check(cx: &mut Cx, node: &Node, kind: Kind, new_name: &str) {
    let call = node.as_call_node().unwrap();
    // on_block: each_with_object, then (Ruby >= 2.6) to_h { }.
    if call.block().is_some_and(|b| b.as_block_node().is_some()) {
        let range = (node.location().start_offset(), node.location().end_offset());
        if let Some(m) = each_with_object(node, kind) {
            register(cx, range, &m, "each_with_object", new_name, 0, 0);
        }
        if cx.config.target_ruby >= 2.6
            && let Some(m) = array_block(node, &["to_h"], kind)
        {
            register(cx, range, &m, "to_h { ... }", new_name, 0, 0);
        }
    }
    // on_send / on_csend: map { }.to_h (the to_h call's receiver is the map block).
    let (s, e) = send_range(&call);
    if method_name(&call) == "to_h"
        && send_args(&call).is_empty()
        && let Some(recv) = call.receiver()
        && let Some(m) = array_block(&recv, &["map", "collect"], kind)
    {
        // from_map_to_h: strip `.to_h` unless the to_h call has a block of its own.
        let has_block = call.block().is_some_and(|b| b.as_block_node().is_some());
        let trailing = if has_block { 0 } else { e - recv.location().end_offset() };
        register(cx, (s, e), &m, "map { ... }.to_h", new_name, 0, trailing);
    }
    // on_send: Hash[map { }]
    if !is_csend(&call)
        && method_name(&call) == "[]"
        && call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Hash"))
        && let [arg] = send_args(&call).as_slice()
        && let Some(m) = array_block(arg, &["map", "collect"], kind)
    {
        let leading = format!("{}[", node_text(&call.receiver().unwrap())).len();
        register(cx, (s, e), &m, "Hash[map { ... }]", new_name, leading, 1);
    }
}
