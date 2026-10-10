use std::cell::Cell;

use ruby_prism::Visit;

use crate::ast::{is_csend, node_text, send_args, send_range};
use crate::helpers::allowed;
use crate::prelude::*;

pub const NAME: &str = "Lint/NumberConversion";
pub static COP: NumberConversion = NumberConversion;
pub struct NumberConversion;

const CONVERSION_METHODS: &[&[u8]] = &[b"Integer", b"Float", b"Complex", b"Rational", b"to_i", b"to_f", b"to_c", b"to_r"];

/// CONVERSION_METHOD_CLASS_MAPPING
fn corrected(method: &[u8], object: &str) -> Option<String> {
    Some(match method {
        b"to_i" => format!("Integer({object}, 10)"),
        b"to_f" => format!("Float({object})"),
        b"to_c" => format!("Complex({object})"),
        b"to_r" => format!("Rational({object})"),
        _ => return None,
    })
}

fn message(current: &str, corrected: &str) -> String {
    format!(
        "Replace unsafe number conversion with number class parsing, instead of using `{current}`, use stricter `{corrected}`."
    )
}

/// Ranges of the sends the corrector rewrote (`ignore_node`).
#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl Cop for NumberConversion {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }
    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = call.name().as_slice();
        let args = send_args(&call);
        if args.is_empty() {
            handle_conversion_method(cx, &call, name);
        } else if args.len() == 1 {
            handle_as_symbol(cx, &call, &args[0]);
        }
    }
}

fn handle_conversion_method(cx: &mut Cx, call: &ruby_prism::CallNode, name: &[u8]) {
    let Some(receiver) = call.receiver() else { return };
    let src = node_text(&receiver);
    let Some(fixed) = corrected(name, src) else { return };
    if allow_receiver(cx, &receiver) {
        return;
    }
    let op = if is_csend(call) { "&." } else { "." };
    let msg = message(&format!("{src}{op}{}", String::from_utf8_lossy(name)), &fixed);
    let (start, end) = send_range(call);
    let skip = is_csend(call) || has_csend(&receiver) || cx.state::<Ignored>().0.iter().any(|&(s, e)| s <= start && e >= end);
    let ignored = Cell::new(false);
    cx.add_fix(start, end, msg, |c| {
        if skip {
            return;
        }
        c.replace(start, end, &fixed);
        ignored.set(true);
    });
    if ignored.get() {
        cx.state::<Ignored>().0.push((start, end));
    }
}

fn handle_as_symbol(cx: &mut Cx, call: &ruby_prism::CallNode, arg: &Node) {
    let sym_value = |n: &Node| n.as_symbol_node().map(|s| s.unescaped().to_vec());
    let to_method = match arg.as_block_argument_node() {
        Some(b) => b.expression().and_then(|e| sym_value(&e)),
        None => sym_value(arg),
    };
    let Some(to_method) = to_method else { return };
    let Some(body) = corrected(&to_method, "i") else { return };
    let replacement = format!("{{ |i| {body} }}");
    let loc = arg.location();
    let (sym_start, sym_end) = (loc.start_offset(), loc.end_offset());
    let msg = message(node_text(arg), &replacement);
    let parens = call
        .opening_loc()
        .filter(|o| o.as_slice() == b"(")
        .zip(call.closing_loc())
        .map(|(o, c)| (o.start_offset(), o.end_offset(), c.start_offset(), c.end_offset()));
    let (start, end) = send_range(call);
    cx.add_fix(start, end, msg, |c| {
        if let Some((os, oe, cs, ce)) = parens {
            c.replace(os, oe, " ");
            c.remove(cs, ce);
        }
        c.replace(sym_start, sym_end, &replacement);
    });
}

/// A parser `send` (a Prism call that owns no `do`/`{}` block).
fn plain_call<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

fn allow_receiver(cx: &Cx, receiver: &Node) -> bool {
    if matches!(
        receiver,
        Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. } | Node::ImaginaryNode { .. }
    ) {
        return true;
    }
    if let Some(c) = plain_call(receiver) {
        let name = c.name().as_slice();
        let name_s = String::from_utf8_lossy(name);
        if CONVERSION_METHODS.contains(&name) || allowed::allowed_method(cx.cfg, &name_s) || allowed::matches_allowed_pattern(cx.cfg, &name_s) {
            return true;
        }
    }
    top_const_name(receiver).is_some_and(|n| cx.cfg.strings("AllowedClasses").contains(&n))
}

/// `top_receiver(node)` (`receiver = receiver.receiver until receiver.receiver.nil?`, over
/// send/csend, blocks of calls and `defs`) when it is a const: its `const_name`.
fn top_const_name(node: &Node) -> Option<String> {
    let next = match node {
        Node::CallNode { .. } => node.as_call_node().unwrap().receiver(),
        Node::DefNode { .. } => node.as_def_node().unwrap().receiver(),
        _ => None,
    };
    match next {
        Some(n) => top_const_name(&n),
        None => const_name(node),
    }
}

/// rubocop-ast `const_name` of a const node.
fn const_name(node: &Node) -> Option<String> {
    if let Some(c) = node.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let p = node.as_constant_path_node()?;
    let name = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    Some(match p.parent() {
        None => name,
        Some(parent) => format!("{}::{name}", const_name(&parent).unwrap_or_default()),
    })
}

struct Csend(bool);

impl<'pr> Visit<'pr> for Csend {
    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        self.0 |= is_csend(node);
        ruby_prism::visit_call_node(self, node);
    }
}

/// `node.each_descendant(:csend).any?` over the send's receiver.
fn has_csend(receiver: &Node) -> bool {
    let mut v = Csend(false);
    v.visit(receiver);
    v.0
}
