//! RuboCop's HashTransformMethod mixin (Style/HashTransformKeys, Style/HashTransformValues).

use crate::prelude::*;
use ruby_prism::{BlockNode, CallNode, Visit};

fn name_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// The parameters node of a block when it has only required parameters (and no block locals).
fn requireds<'pr>(block: &BlockNode<'pr>) -> Option<Vec<Node<'pr>>> {
    let bp = block.parameters()?.as_block_parameters_node()?;
    let p = bp.parameters()?;
    let ok = bp.locals().iter().next().is_none()
        && p.optionals().iter().next().is_none()
        && crate::ast::parser_rest(p.rest()).is_none()
        && p.posts().iter().next().is_none()
        && p.keywords().iter().next().is_none()
        && p.keyword_rest().is_none()
        && p.block().is_none();
    ok.then(|| p.requireds().iter().collect())
}

fn req_name(n: &Node) -> Option<String> {
    Some(name_of(n.as_required_parameter_node()?.name().as_slice()))
}

/// `(args (arg a) (arg b))`.
fn two_args(block: &BlockNode) -> Option<(String, String)> {
    match requireds(block)?.as_slice() {
        [a, b] => Some((req_name(a)?, req_name(b)?)),
        _ => None,
    }
}

/// `(args (mlhs (arg a) (arg b)) (arg memo))`.
fn mlhs_args(block: &BlockNode) -> Option<(String, String, String)> {
    let reqs = requireds(block)?;
    let [m, memo] = reqs.as_slice() else { return None };
    let m = m.as_multi_target_node()?;
    if crate::ast::parser_rest(m.rest()).is_some() || m.rights().iter().next().is_some() {
        return None;
    }
    let lefts: Vec<Node> = m.lefts().iter().collect();
    let [a, b] = lefts.as_slice() else { return None };
    Some((req_name(a)?, req_name(b)?, req_name(memo)?))
}

/// A parser `block` (not numblock/itblock) attached to `call`.
fn plain_block<'pr>(call: &CallNode<'pr>) -> Option<BlockNode<'pr>> {
    call.block()?.as_block_node().filter(|b| {
        b.parameters().is_none_or(|p| p.as_numbered_parameters_node().is_none() && p.as_it_parameters_node().is_none())
    })
}

/// The single statement of a block body.
fn single_body<'pr>(block: &BlockNode<'pr>) -> Option<Node<'pr>> {
    let s = block.body()?.as_statements_node()?;
    let mut it = s.body().iter();
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

fn empty_hash(node: &Node) -> bool {
    node.as_hash_node().is_some_and(|h| h.elements().iter().next().is_none())
}

fn lvar_named(node: &Node, name: &str) -> bool {
    node.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name.as_bytes())
}

/// `#hash_receiver?`
fn hash_receiver(node: &Node) -> bool {
    if node.as_hash_node().is_some() {
        return true;
    }
    let Some(call) = node.as_call_node() else { return false };
    if is_csend(&call) {
        return false;
    }
    let name = method_name(&call);
    if call.block().is_some_and(|b| b.as_block_node().is_some()) {
        return plain_block(&call).is_some()
            && match name {
                "group_by" | "to_h" | "tally" | "transform_keys" | "transform_keys!" | "transform_values"
                | "transform_values!" => call.arguments().is_none(),
                "each_with_object" => matches!(call_args(&call).as_slice(), [h] if empty_hash(h)),
                _ => false,
            };
    }
    matches!(name, "to_h" | "to_hash" | "merge" | "merge!" | "update" | "invert" | "except" | "tally")
}

#[derive(Default)]
struct Lvars(Vec<Vec<u8>>);

impl<'pr> Visit<'pr> for Lvars {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.0.push(node.name().as_slice().to_vec());
    }
}

/// Names of `lvar` descendants (excluding the node itself).
fn lvar_descendants(node: &Node) -> Vec<Vec<u8>> {
    if node.as_local_variable_read_node().is_some() {
        return Vec::new();
    }
    let mut v = Lvars::default();
    v.visit(node);
    v.0
}

/// ``!`memo``: the expression mentions the name nowhere (any symbol child in parser).
// ponytail: source word match; a string literal containing the bare word also counts.
fn mentions(src: &str, name: &str) -> bool {
    let bytes = src.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80;
    src.match_indices(name).any(|(i, _)| {
        let before = i.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(i + name.len()).copied();
        !before.is_some_and(|c| word(c) || c == b'@' || c == b'$')
            && !after.is_some_and(|c| word(c) || c == b'?' || c == b'!' || c == b'=')
    })
}

/// One `on_bad_*` match: the block (call + BlockNode) being rewritten, the offense node, and
/// the transformed parameter, transforming and unchanged expressions (Captures).
struct Match<'a, 'pr> {
    call: &'a CallNode<'pr>,
    block: &'a BlockNode<'pr>,
    node: (usize, usize),
    leading: usize,
    trailing: usize,
    argname: String,
    transforming: Node<'pr>,
    unchanged: Node<'pr>,
    desc: &'static str,
}

fn handle(cx: &mut Cx, m: Match, keys: bool) {
    let t = &m.transforming;
    // noop_transformation?
    if lvar_named(t, &m.argname) {
        return;
    }
    let lvars = lvar_descendants(t);
    // transformation_uses_both_args?: the unchanged `(lvar x)` is a descendant.
    let unchanged = m.unchanged.as_local_variable_read_node().map(|l| l.name().as_slice().to_vec());
    if unchanged.is_some_and(|u| lvars.contains(&u)) {
        return;
    }
    // use_transformed_argname?
    if !lvars.iter().any(|n| n == m.argname.as_bytes()) || t.as_splat_node().is_some() {
        return;
    }
    let new_name = if keys { "transform_keys" } else { "transform_values" };
    let msg = format!("Prefer `{new_name}` over `{}`.", m.desc);
    let src = cx.src;
    let sel = m.call.message_loc().map(|l| l.start_offset()).unwrap_or(0);
    let sel_end = match m.call.closing_loc() {
        Some(c) => c.end_offset(),
        None => m.call.message_loc().map(|l| l.end_offset()).unwrap_or(0),
    };
    let args = m.block.parameters().map(|p| range(&p)).unwrap_or((0, 0));
    let body = m.block.body().map(|b| range(&b)).unwrap_or((0, 0));
    let mut body_source = src.slice(range(t).0, range(t).1).to_string();
    if t.as_keyword_hash_node().is_some() {
        body_source = format!("{{ {body_source} }}");
    }
    let new_args = format!("|{}|", m.argname);
    let (node, leading, trailing) = (m.node, m.leading, m.trailing);
    cx.add_fix(node.0, node.1, msg, |c| {
        c.remove_leading(node.0, node.1, leading);
        c.remove_trailing(node.0, node.1, trailing);
        c.replace(sel, sel_end, new_name);
        c.replace(args.0, args.1, &new_args);
        c.replace(body.0, body.1, &body_source);
    });
}

/// `(block (call #hash_receiver? {names}) (args (arg a) (arg b)) (array x y))`: (a, b, x, y).
fn array_block<'pr>(call: &CallNode<'pr>, block: &BlockNode<'pr>, names: &[&str]) -> Option<(String, String, Node<'pr>, Node<'pr>)> {
    if !names.contains(&method_name(call)) || call.arguments().is_some() || !call.receiver().is_some_and(|r| hash_receiver(&r)) {
        return None;
    }
    let (a, b) = two_args(block)?;
    let arr = single_body(block)?;
    let arr = arr.as_array_node()?;
    let mut it = arr.elements().iter();
    let (x, y) = (it.next()?, it.next()?);
    it.next().is_none().then_some((a, b, x, y))
}

/// Builds the Captures for an `(array x y)` match: Keys transform `x` (unchanged `y` must be the
/// value parameter), Values the other way round.
#[allow(clippy::too_many_arguments)]
fn array_match<'a, 'pr>(
    call: &'a CallNode<'pr>,
    block: &'a BlockNode<'pr>,
    (a, b, x, y): (String, String, Node<'pr>, Node<'pr>),
    keys: bool,
    node: (usize, usize),
    leading: usize,
    trailing: usize,
    desc: &'static str,
) -> Option<Match<'a, 'pr>> {
    let (argname, transforming, unchanged) = if keys {
        lvar_named(&y, &b).then_some((a, x, y))?
    } else {
        lvar_named(&x, &a).then_some((b, y, x))?
    };
    Some(Match { call, block, node, leading, trailing, argname, transforming, unchanged, desc })
}

pub fn check(cx: &mut Cx, node: &Node, keys: bool) {
    let call = node.as_call_node().unwrap();
    let whole = range(node);

    if let Some(block) = plain_block(&call) {
        // on_bad_each_with_object
        if method_name(&call) == "each_with_object"
            && matches!(call_args(&call).as_slice(), [h] if empty_hash(h))
            && call.receiver().is_some_and(|r| hash_receiver(&r))
            && let Some((k, v, memo)) = mlhs_args(&block)
            && let Some(body) = single_body(&block)
            && let Some(asgn) = body.as_call_node()
            && method_name(&asgn) == "[]="
            && asgn.block().is_none()
            && asgn.receiver().is_some_and(|r| lvar_named(&r, &memo))
            && let Some(args) = asgn.arguments()
        {
            let mut it = args.arguments().iter();
            if let (Some(key), Some(val), None) = (it.next(), it.next(), it.next()) {
                let src = cx.src;
                let m = if keys {
                    (lvar_named(&val, &v) && !mentions(src.slice(range(&key).0, range(&key).1), &memo))
                        .then_some((k, key, val))
                } else {
                    (lvar_named(&key, &k) && !mentions(src.slice(range(&val).0, range(&val).1), &memo))
                        .then_some((v, val, key))
                };
                if let Some((argname, transforming, unchanged)) = m {
                    let desc = "each_with_object";
                    let m = Match { call: &call, block: &block, node: whole, leading: 0, trailing: 0, argname, transforming, unchanged, desc };
                    handle(cx, m, keys);
                }
            }
        }
        // on_bad_to_h (Ruby 2.6+)
        if cx.config.target_ruby >= 2.6
            && let Some(parts) = array_block(&call, &block, &["to_h"])
            && let Some(m) = array_match(&call, &block, parts, keys, whole, 0, 0, "to_h {...}")
        {
            handle(cx, m, keys);
        }
    }

    // on_send: Hash[_.map {...}]
    if method_name(&call) == "[]"
        && !is_csend(&call)
        && call.block().is_none()
        && call.receiver().is_some_and(|r| {
            r.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Hash")
                || r.as_constant_path_node().is_some_and(|c| c.name().is_some_and(|n| n.as_slice() == b"Hash"))
        })
        && let [arg] = call_args(&call).as_slice()
        && let Some(map) = arg.as_call_node()
        && let Some(block) = plain_block(&map)
        && let Some(parts) = array_block(&map, &block, &["map", "collect"])
        && let Some(m) = array_match(&map, &block, parts, keys, whole, 5, 1, "Hash[_.map {...}]")
    {
        handle(cx, m, keys);
    }

    // on_send / on_csend: map {...}.to_h
    if method_name(&call) == "to_h"
        && call.arguments().is_none()
        && call.block().is_none_or(|b| b.as_block_node().is_some())
        && let Some(map) = call.receiver().and_then(|r| r.as_call_node())
        && let Some(block) = plain_block(&map)
        && let Some(parts) = array_block(&map, &block, &["map", "collect"])
    {
        let node = send_range(&call);
        let trailing = if call.block().is_some() { 0 } else { node.1 - map.location().end_offset() };
        if let Some(m) = array_match(&map, &block, parts, keys, node, 0, trailing, "map {...}.to_h") {
            handle(cx, m, keys);
        }
    }
}
