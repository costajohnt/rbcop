use crate::cop::Corrector;
use crate::helpers::frozen_string_literal as fsl;
use crate::helpers::magic_comment::shareable_constant_value;
use crate::helpers::parser_parent::{is_ternary, is_top_const};
use crate::prelude::*;

pub const NAME: &str = "Style/MutableConstant";
pub static COP: MutableConstant = MutableConstant;
pub struct MutableConstant;

const MSG: &str = "Freeze mutable objects assigned to constants.";

struct Ctx<'a, 'b, 'c> {
    cx: &'a Cx<'b, 'c>,
    ruby3: bool,
    recursive: bool,
}

fn is_range_or_regexp(node: &Node) -> bool {
    matches!(
        node,
        Node::RangeNode { .. } | Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. }
    )
}

/// rubocop-ast `mutable_literal?`: str dstr xstr array hash regexp irange erange.
fn ast_mutable_literal(node: &Node) -> bool {
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
    ) || is_range_or_regexp(node)
}

/// rubocop-ast `immutable_literal?`: int float sym dsym true false nil complex rational.
fn ast_immutable_literal(node: &Node) -> bool {
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

/// A parser `send` (not csend, not a block).
fn send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// `(begin (range _ _))`
fn range_in_parens(node: &Node) -> bool {
    let Some(body) = node.as_parentheses_node().and_then(|p| p.body()) else { return false };
    match body.as_statements_node() {
        Some(s) => {
            let v: Vec<Node> = s.body().iter().collect();
            v.len() == 1 && v[0].as_range_node().is_some()
        }
        None => body.as_range_node().is_some(),
    }
}

fn is_number(node: &Node) -> bool {
    matches!(crate::helpers::literal_node::folded_kind(node), kind::INTEGER_NODE | kind::FLOAT_NODE)
}

fn env_index(node: &Node) -> bool {
    send(node).is_some_and(|c| {
        method_name(&c) == "[]" && send_args(&c).len() == 1 && c.receiver().is_some_and(|r| is_top_const(&r, "ENV"))
    })
}

fn operation_produces_immutable_object(node: &Node) -> bool {
    if matches!(node, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }) {
        return true;
    }
    if let Some(o) = node.as_or_node() {
        return env_index(&o.left());
    }
    // (block (send ...) ...): a call with a literal block
    if let Some(c) = node.as_call_node()
        && !is_csend(&c)
        && c.block().is_some_and(|b| b.as_block_node().is_some())
        && c.block().unwrap().as_block_node().unwrap().parameters().is_none_or(|p| p.as_block_parameters_node().is_some())
    {
        return struct_or_data(&c) || matches!(method_name(&c), "count" | "length" | "size");
    }
    let Some(c) = send(node) else { return false };
    if struct_or_data(&c) || env_index(node) {
        return true;
    }
    let name = method_name(&c);
    let args = send_args(&c);
    let recv = c.receiver();
    match name {
        "freeze" => args.is_empty(),
        "count" | "length" | "size" => true,
        "==" | "===" | "!=" | "<=" | ">=" | "<" | ">" => args.len() == 1,
        "+" | "-" | "*" | "**" | "/" | "%" | "<<" => {
            args.len() == 1
                && (recv.as_ref().is_some_and(is_number) || (name != "<<" && is_number(&args[0])))
        }
        _ => false,
    }
}

/// `(send (const {nil? cbase} :Struct) :new ...)` / `(send (const {nil? cbase} :Data) :define ...)`
fn struct_or_data(c: &ruby_prism::CallNode) -> bool {
    let Some(r) = c.receiver() else { return false };
    match method_name(c) {
        "new" => is_top_const(&r, "Struct"),
        "define" => is_top_const(&r, "Data"),
        _ => false,
    }
}

impl Ctx<'_, '_, '_> {
    fn frozen_regexp_or_range(&self, node: &Node) -> bool {
        self.ruby3 && is_range_or_regexp(node)
    }

    fn mutable_literal(&self, node: &Node) -> bool {
        !self.frozen_regexp_or_range(node) && ast_mutable_literal(node)
    }

    fn shareable(&self, node: &Node) -> bool {
        if !self.ruby3 {
            return false;
        }
        let src = self.cx.src;
        let last = src.line_of(node.location().end_offset());
        src.lines[..last.min(src.lines.len())].iter().rev().find_map(|l| {
            shareable_constant_value(l).filter(|v| matches!(v.as_str(), "none" | "literal" | "experimental_everything" | "experimental_copy"))
        })
        .is_some_and(|v| v != "none")
    }

    fn check(&self, node: &Node, strict: bool) -> bool {
        if strict {
            return self.strict_check(node);
        }
        if !(self.mutable_literal(node) || (self.cx.config.target_ruby <= 2.7 && range_in_parens(node))) {
            return false;
        }
        !fsl::frozen_string_literal(self.cx, node) && !self.shareable(node)
    }

    fn strict_check(&self, node: &Node) -> bool {
        !(is_assignment(node)
            || self.frozen_regexp_or_range(node)
            || ast_immutable_literal(node)
            || operation_produces_immutable_object(node)
            || fsl::frozen_string_literal(self.cx, node)
            || self.shareable(node)
            || self.parenthesized_immutable_value(node))
    }

    /// `(begin x)` around a value strict mode accepts (1.92).
    fn parenthesized_immutable_value(&self, node: &Node) -> bool {
        let Some(body) = node.as_parentheses_node().and_then(|p| p.body()) else { return false };
        let inner = match body.as_statements_node() {
            Some(s) => {
                let mut v = s.body().iter();
                let (Some(only), None) = (v.next(), v.next()) else { return false };
                only
            }
            None => body,
        };
        !is_assignment(&inner) && !self.strict_check(&inner)
    }

    fn explicitly_frozen_literal<'pr>(&self, node: &Node<'pr>) -> Option<Node<'pr>> {
        let c = send(node)?;
        if method_name(&c) != "freeze" {
            return None;
        }
        c.receiver().filter(|r| self.mutable_literal(r))
    }

    fn mutable_nodes<'pr>(&self, value: Node<'pr>, strict: bool, out: &mut Vec<Node<'pr>>) {
        if self.recursive
            && let Some(recv) = self.explicitly_frozen_literal(&value)
        {
            for c in literal_children(&recv) {
                self.mutable_nodes(c, strict, out);
            }
        } else if self.check(&value, strict) {
            out.push(value);
        }
    }

    /// The autocorrect block's edits, computed up front.
    fn autocorrect(&self, node: &Node, edits: &mut Vec<Edit>) {
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        if let Some(a) = node.as_array_node() {
            let elems: Vec<Node> = a.elements().iter().collect();
            if let [only] = elems.as_slice()
                && let Some(splat) = only.as_splat_node()
                && let Some(v) = splat.expression()
            {
                let src = node_text(&v);
                let text = if range_in_parens(&v) { format!("{src}.to_a") } else { format!("({src}).to_a") };
                edits.push(Edit::Replace(s, e, text));
                edits.push(Edit::InsertAfter(s, e, ".freeze".into()));
                return;
            }
            if a.opening_loc().is_none() {
                edits.push(Edit::Wrap(s, e, "[", "]"));
            }
        } else if node.as_implicit_node().is_some() {
            // `{ a: }`: expand the omitted value (1.92). Prism's implicit value spans the `a:` pair.
            let key = node_text(node).trim_end_matches(':');
            edits.push(Edit::Replace(s, e, format!("{key}: {key}.freeze")));
            return;
        } else if node.as_keyword_hash_node().is_some() {
            edits.push(Edit::Wrap(s, e, "{ ", " }"));
        } else if requires_parentheses(node) {
            edits.push(Edit::Wrap(s, e, "(", ")"));
        }
        edits.push(Edit::InsertAfter(s, e, ".freeze".into()));
        if self.recursive {
            self.freeze_nested_literals(node, edits);
        }
    }

    fn freeze_nested_literals(&self, node: &Node, edits: &mut Vec<Edit>) {
        for child in literal_children(node) {
            if let Some(recv) = self.explicitly_frozen_literal(&child) {
                self.freeze_nested_literals(&recv, edits);
            } else if !fsl::frozen_string_literal(self.cx, &child) && !self.shareable(&child) && self.mutable_literal(&child) {
                self.autocorrect(&child, edits);
            }
        }
    }
}

enum Edit {
    Replace(usize, usize, String),
    InsertAfter(usize, usize, String),
    Wrap(usize, usize, &'static str, &'static str),
}

fn apply(c: &mut Corrector, edits: &[Edit]) {
    for e in edits {
        match e {
            Edit::Replace(s, e, t) => c.replace(*s, *e, t),
            Edit::InsertAfter(s, e, t) => c.insert_after(*s, *e, t),
            Edit::Wrap(s, e, a, b) => c.wrap(*s, *e, a, b),
        }
    }
}

/// `requires_parentheses?` (1.92): ranges, `and`/`or`, ternaries and calls without a dot or
/// with unparenthesized arguments.
fn requires_parentheses(node: &Node) -> bool {
    if node.as_range_node().is_some() || node.as_and_node().is_some() || node.as_or_node().is_some() || is_ternary(node) {
        return true;
    }
    let Some(c) = node.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none())) else { return false };
    c.call_operator_loc().is_none() || (!send_args(&c).is_empty() && c.opening_loc().is_none())
}

/// literal_children: array elements (not %w/%i), hash pair keys and values.
fn literal_children<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    if let Some(a) = node.as_array_node() {
        if a.opening_loc().is_some_and(|o| o.as_slice().starts_with(b"%")) {
            return Vec::new();
        }
        return a.elements().iter().filter(|e| e.as_splat_node().is_none()).collect();
    }
    let elements = node.as_hash_node().map(|h| h.elements()).or_else(|| node.as_keyword_hash_node().map(|h| h.elements()));
    if let Some(elements) = elements {
        return elements
            .iter()
            .filter_map(|e| e.as_assoc_node())
            .flat_map(|p| [p.key(), p.value()])
            .collect();
    }
    Vec::new()
}

impl Cop for MutableConstant {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::CONSTANT_OR_WRITE_NODE,
            kind::CONSTANT_PATH_OR_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let value = match node {
            Node::ConstantWriteNode { .. } => node.as_constant_write_node().unwrap().value(),
            Node::ConstantPathWriteNode { .. } => node.as_constant_path_write_node().unwrap().value(),
            Node::ConstantOrWriteNode { .. } => node.as_constant_or_write_node().unwrap().value(),
            _ => node.as_constant_path_or_write_node().unwrap().value(),
        };
        let strict = cx.cfg.str("EnforcedStyle") == Some("strict");
        let ctx = Ctx { cx: &*cx, ruby3: cx.config.target_ruby >= 3.0, recursive: cx.cfg.bool("Recursive") == Some(true) };
        let mut nodes = Vec::new();
        ctx.mutable_nodes(value, strict, &mut nodes);
        let fixes: Vec<((usize, usize), Vec<Edit>)> = nodes
            .iter()
            .map(|n| {
                let mut edits = Vec::new();
                ctx.autocorrect(n, &mut edits);
                // parser's omitted value is the bare key (`a` of `a:`).
                let l = n.location();
                let trim = usize::from(n.as_implicit_node().is_some());
                ((l.start_offset(), l.end_offset() - trim), edits)
            })
            .collect();
        for ((s, e), edits) in fixes {
            cx.add_fix(s, e, MSG, |c| apply(c, &edits));
        }
    }
}
