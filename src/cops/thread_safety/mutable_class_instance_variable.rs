use crate::helpers::frozen_string_literal as fsl;
use crate::helpers::literal_node::folded_kind;
use crate::helpers::node_types::plain_block_params;
use crate::helpers::parser_parent::is_top_const;
use crate::helpers::thread_safety::{self as ts, Anc, const_name, operation_produces_threadsafe_object};
use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "ThreadSafety/MutableClassInstanceVariable";
pub static COP: MutableClassInstanceVariable = MutableClassInstanceVariable;
pub struct MutableClassInstanceVariable;

const MSG: &str = "Freeze mutable objects assigned to class instance variables.";

/// rubocop-ast `mutable_literal?` (str dstr xstr array hash regexp irange erange); the cop adds
/// `range` types, already in that list.
fn mutable_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::StringNode { .. }
            | Node::SourceFileNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::XStringNode { .. }
            | Node::InterpolatedXStringNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::KeywordHashNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::InterpolatedRegularExpressionNode { .. }
            | Node::RangeNode { .. }
    )
}

/// rubocop-ast `immutable_literal?`: int float sym dsym true false nil complex rational.
fn immutable_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::IntegerNode { .. }
            | Node::SourceLineNode { .. }
            | Node::FloatNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::RationalNode { .. }
    ) || crate::helpers::literal_node::folded_numeric(node).is_some()
}

/// The single statement of a parser `(begin x)` (parentheses around one expression).
fn paren_body<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let body = node.as_parentheses_node()?.body()?;
    match body.as_statements_node() {
        Some(s) => {
            let mut it = s.body().iter();
            match (it.next(), it.next()) {
                (Some(only), None) => Some(only),
                _ => None,
            }
        }
        None => Some(body),
    }
}

/// `(begin (range _ _))`
fn range_in_parens(node: &Node) -> bool {
    paren_body(node).is_some_and(|b| b.as_range_node().is_some())
}

/// A parser `send` (not csend, not a block).
fn send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// A parser `block` (ordinary parameters) of a `send`: its call.
fn block_send<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| {
        !is_csend(c) && c.block().and_then(|b| b.as_block_node()).is_some_and(|b| plain_block_params(b.parameters()))
    })
}

fn is_number(node: &Node) -> bool {
    matches!(folded_kind(node), kind::INTEGER_NODE | kind::FLOAT_NODE)
}

/// `(send (const {nil? cbase} :ENV) :[] _)`
fn env_index(node: &Node) -> bool {
    send(node).is_some_and(|c| {
        method_name(&c) == "[]" && send_args(&c).len() == 1 && c.receiver().is_some_and(|r| is_top_const(&r, "ENV"))
    })
}

fn struct_new(c: &CallNode) -> bool {
    method_name(c) == "new" && c.receiver().is_some_and(|r| is_top_const(&r, "Struct"))
}

/// operation_produces_immutable_object?
fn operation_produces_immutable_object(node: &Node) -> bool {
    if matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }) {
        return true;
    }
    if let Some(o) = node.as_or_node() {
        return env_index(&o.left());
    }
    if let Some(c) = block_send(node) {
        return struct_new(&c) || matches!(method_name(&c), "count" | "length" | "size");
    }
    let Some(c) = send(node) else { return false };
    if struct_new(&c) || env_index(node) {
        return true;
    }
    let name = method_name(&c);
    let args = send_args(&c);
    match name {
        "freeze" => args.is_empty(),
        "count" | "length" | "size" => true,
        "==" | "===" | "!=" | "<=" | ">=" | "<" | ">" => args.len() == 1,
        "+" | "-" | "*" | "**" | "/" | "%" | "<<" => {
            args.len() == 1 && (c.receiver().is_some_and(|r| is_number(&r)) || (name != "<<" && is_number(&args[0])))
        }
        _ => false,
    }
}

/// The cop's own `frozen_string_literal?`: a `str` (and a `dstr` below Ruby 3.0) with frozen
/// string literals enabled.
fn frozen_string_literal(cx: &Cx, node: &Node) -> bool {
    let dstr = is_parser_dstr(node);
    let str_type = matches!(node, Node::StringNode { .. } | Node::SourceFileNode { .. }) && !dstr;
    (str_type || (dstr && cx.config.target_ruby < 3.0)) && fsl::enabled(cx)
}

/// `requires_parentheses?`: a range, or a `send` without a dot or with unparenthesized arguments.
fn requires_parentheses(node: &Node) -> bool {
    if node.as_range_node().is_some() {
        return true;
    }
    send(node).is_some_and(|c| c.call_operator_loc().is_none() || (!send_args(&c).is_empty() && c.opening_loc().is_none()))
}

fn autocorrect(c: &mut Corrector, node: &Node) {
    let loc = node.location();
    let (s, e) = (loc.start_offset(), loc.end_offset());
    if let Some(a) = node.as_array_node() {
        let elems: Vec<Node> = a.elements().iter().collect();
        if let [only] = elems.as_slice()
            && let Some(v) = only.as_splat_node().and_then(|s| s.expression())
        {
            let src = node_text(&v);
            c.replace(s, e, &if range_in_parens(&v) { format!("{src}.to_a") } else { format!("({src}).to_a") });
        } else if a.opening_loc().is_none() {
            c.wrap(s, e, "[", "]");
        }
    } else if requires_parentheses(node) {
        c.wrap(s, e, "(", ")");
    }
    c.insert_after(s, e, ".freeze");
}

/// in_class?: the nearest def, defs, class, module or define(_singleton)_method block is a class
/// or module.
fn in_class(ancs: &[Anc]) -> bool {
    ancs.iter()
        .find_map(|a| match a {
            Anc::Node(n) => match n {
                Node::ClassNode { .. } | Node::ModuleNode { .. } => Some(true),
                Node::DefNode { .. } => Some(false),
                _ => None,
            },
            Anc::Block { plain: true, .. } => a
                .block_call()
                .filter(|c| c.receiver().is_none() && matches!(method_name(c), "define_method" | "define_singleton_method"))
                .map(|_| false),
            Anc::Block { .. } => None,
        })
        .unwrap_or(false)
}

/// within_dsl_with_threadsafe_semantics?: inside a `setup`/`teardown` block of a class whose
/// superclass is `ActiveSupport::TestCase`.
fn within_threadsafe_dsl(ancs: &[Anc]) -> bool {
    let Some(i) = ancs.iter().position(Anc::is_block) else { return false };
    let name = match &ancs[i] {
        Anc::Block { owner, .. } => match owner {
            Node::CallNode { .. } => method_name(&owner.as_call_node().unwrap()),
            Node::LambdaNode { .. } => "lambda",
            _ => "super",
        },
        Anc::Node(_) => return false,
    };
    if !matches!(name, "setup" | "teardown") {
        return false;
    }
    ancs[i + 1..]
        .iter()
        .find_map(|a| a.node().and_then(|n| n.as_class_node()))
        .and_then(|c| c.superclass())
        .and_then(|s| const_name(&s))
        .is_some_and(|n| n == "ActiveSupport::TestCase")
}

impl MutableClassInstanceVariable {
    fn offense(&self, cx: &Cx, ancs: &[Anc], value: &Node) -> bool {
        if within_threadsafe_dsl(ancs) {
            return false;
        }
        if cx.cfg.params.str("EnforcedStyle") == Some("strict") {
            !(immutable_literal(value)
                || operation_produces_immutable_object(value)
                || operation_produces_threadsafe_object(value)
                || frozen_string_literal(cx, value))
        } else {
            (mutable_literal(value) || range_in_parens(value)) && !frozen_string_literal(cx, value)
        }
    }
}

impl Cop for MutableClassInstanceVariable {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::INSTANCE_VARIABLE_WRITE_NODE, kind::INSTANCE_VARIABLE_OR_WRITE_NODE, kind::MULTI_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let ancs = ts::parser_ancestors(cx.ancestors);
        if !in_class(&ancs) {
            return;
        }
        let values: Vec<Node> = match node {
            Node::InstanceVariableWriteNode { .. } => vec![node.as_instance_variable_write_node().unwrap().value()],
            Node::InstanceVariableOrWriteNode { .. } => vec![node.as_instance_variable_or_write_node().unwrap().value()],
            _ => {
                // on_masgn: `mlhs.to_a.zip(values.to_a)` when the right side is an array.
                let m = node.as_multi_write_node().unwrap();
                let Some(array) = m.value().as_array_node() else { return };
                let rest = m.rest().filter(|r| r.as_implicit_rest_node().is_none());
                let lhs: Vec<Node> = m.lefts().iter().chain(rest).chain(m.rights().iter()).collect();
                array
                    .elements()
                    .iter()
                    .zip(lhs)
                    .filter(|(_, l)| l.as_instance_variable_target_node().is_some())
                    .map(|(v, _)| v)
                    .collect()
            }
        };
        let flagged: Vec<Node> = values.into_iter().filter(|v| self.offense(cx, &ancs, v)).collect();
        for v in flagged {
            let l = v.location();
            cx.add_fix(l.start_offset(), l.end_offset(), MSG, |c| autocorrect(c, &v));
        }
    }
}
