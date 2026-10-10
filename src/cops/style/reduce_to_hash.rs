use crate::prelude::*;
use ruby_prism::{BlockNode, CallNode, Visit};

pub const NAME: &str = "Style/ReduceToHash";
pub static COP: ReduceToHash = ReduceToHash;
pub struct ReduceToHash;

struct Match<'pr> {
    key: Node<'pr>,
    value: Node<'pr>,
    /// accumulator name (`_hash`, or `_1`/`_2` for a numblock)
    acc: Vec<u8>,
    /// element argument source (named blocks)
    elem: Option<&'pr str>,
    numbered: bool,
}

fn lvar_named(n: &Node, name: &[u8]) -> bool {
    n.as_local_variable_read_node().is_some_and(|l| l.name().as_slice() == name)
}

/// `(send (lvar acc) :[]= $key $value)`
fn store<'pr>(n: &Node<'pr>, acc: &[u8]) -> Option<(Node<'pr>, Node<'pr>)> {
    let c = n.as_call_node()?;
    if method_name(&c) != "[]=" || is_csend(&c) || c.block().is_some() || !c.receiver().is_some_and(|r| lvar_named(&r, acc)) {
        return None;
    }
    let mut args = call_args(&c).into_iter();
    match (args.next(), args.next(), args.next()) {
        (Some(k), Some(v), None) => Some((k, v)),
        _ => None,
    }
}

/// `(args (arg _) (arg _))`: exactly two plain block parameters.
fn named_params<'pr>(block: &BlockNode<'pr>) -> Option<Vec<ruby_prism::RequiredParameterNode<'pr>>> {
    let bp = block.parameters()?.as_block_parameters_node()?;
    if bp.locals().iter().next().is_some() {
        return None;
    }
    let p = bp.parameters()?;
    if p.optionals().iter().next().is_some()
        || p.rest().is_some()
        || p.posts().iter().next().is_some()
        || p.keywords().iter().next().is_some()
        || p.keyword_rest().is_some()
        || p.block().is_some()
    {
        return None;
    }
    let req: Vec<_> = p.requireds().iter().filter_map(|r| r.as_required_parameter_node()).collect();
    (req.len() == 2 && p.requireds().iter().count() == 2).then_some(req)
}

fn matches<'pr>(call: &CallNode<'pr>) -> Option<Match<'pr>> {
    let name = method_name(call);
    let ewo = name == "each_with_object";
    if !(ewo || name == "inject" || name == "reduce") {
        return None;
    }
    let block = call.block()?.as_block_node()?;
    let mut args = send_args(call).into_iter();
    let (Some(h), None) = (args.next(), args.next()) else { return None };
    if !h.as_hash_node().is_some_and(|h| h.elements().iter().next().is_none()) {
        return None;
    }
    let stmts: Vec<Node> = block.body()?.as_statements_node()?.body().iter().collect();
    let numbered = block.parameters().and_then(|p| p.as_numbered_parameters_node());
    let (acc, elem): (Vec<u8>, Option<&str>) = if let Some(n) = &numbered {
        if n.maximum() != 2 {
            return None;
        }
        (if ewo { b"_2".to_vec() } else { b"_1".to_vec() }, None)
    } else {
        let params = named_params(&block)?;
        let (acc, elem) = if ewo { (&params[1], &params[0]) } else { (&params[0], &params[1]) };
        (acc.name().as_slice().to_vec(), Some(text(&elem.location())))
    };
    let (key, value) = if ewo {
        if stmts.len() != 1 {
            return None;
        }
        store(&stmts[0], &acc)?
    } else {
        if stmts.len() != 2 || !lvar_named(&stmts[1], &acc) {
            return None;
        }
        store(&stmts[0], &acc)?
    };
    Some(Match { key, value, acc, elem, numbered: numbered.is_some() })
}

/// `node.each_node(:lvar).any? { name }` and `nested_match?` over a key/value expression.
struct Scan<'a> {
    acc: &'a [u8],
    uses_acc: bool,
    nested: bool,
}

impl<'pr> Visit<'pr> for Scan<'_> {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        if node.name().as_slice() == self.acc {
            self.uses_acc = true;
        }
    }
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if matches(node).is_some() {
            self.nested = true;
        }
        ruby_prism::visit_call_node(self, node);
    }
}

fn scan(n: &Node, acc: &[u8]) -> (bool, bool) {
    let mut s = Scan { acc, uses_acc: false, nested: false };
    s.visit(n);
    (s.uses_acc, s.nested)
}

impl Cop for ReduceToHash {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.6, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(m) = matches(&call) else { return };
        let (key_acc, key_nested) = scan(&m.key, &m.acc);
        let (value_acc, value_nested) = scan(&m.value, &m.acc);
        if key_acc || value_acc || key_nested || value_nested {
            return;
        }
        let Some(sel) = call.message_loc() else { return };
        let block = call.block().unwrap().as_block_node().unwrap();
        let name = method_name(&call);
        let adjust = |n: &Node| {
            let s = node_text(n);
            if m.numbered && name != "each_with_object" { s.replace("_2", "_1") } else { s.to_string() }
        };
        let body = format!("[{}, {}]", adjust(&m.key), adjust(&m.value));
        let braces = block.opening_loc().as_slice() == b"{";
        let indent = " ".repeat(cx.src.column(node.location().start_offset()));
        let do_end = |arg: Option<&str>| {
            let args = arg.map_or(String::new(), |a| format!(" |{a}|"));
            format!("to_h do{args}\n{indent}  {body}\n{indent}end")
        };
        let replacement = match (m.elem, braces) {
            (None, true) => format!("to_h {{ {body} }}"),
            (Some(a), true) => format!("to_h {{ |{a}| {body} }}"),
            (arg, false) => do_end(arg),
        };
        let (start, end) = (sel.start_offset(), sel.end_offset());
        let block_end = node.location().end_offset();
        let msg = format!("Use `to_h {{ ... }}` instead of `{name}`.");
        cx.add_fix(start, end, msg, |c| c.replace(start, block_end, &replacement));
    }
}
