use crate::helpers::node_types::{is_assignment, is_operator_method};
use crate::helpers::parser_parent::{is_logical_operator, is_operator_keyword, is_ternary, parser_parent};
use crate::prelude::*;
use ruby_prism::Location;

pub const NAME: &str = "Style/NestedModifier";
pub static COP: NestedModifier = NestedModifier;
pub struct NestedModifier;

#[derive(Default)]
struct Ignored(Vec<(usize, usize)>);

impl Cop for NestedModifier {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::IF_NODE, kind::UNLESS_NODE, kind::WHILE_NODE, kind::UNTIL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        if cx.state::<Ignored>().0.iter().any(|&(a, b)| a <= s && e <= b) {
            return;
        }
        if is_pattern_guard(node, cx.ancestors) {
            return;
        }
        let Some(inner) = modifier(node) else { return };
        let Some(parent) = parser_parent(cx.ancestors) else { return };
        let Some(outer) = modifier(parent) else { return };
        let fix = (inner.is_if && outer.is_if).then(|| {
            let range = (inner.keyword.start_offset(), outer.condition.location().end_offset());
            (range, new_expression(&inner, &outer))
        });
        let kw = inner.keyword;
        cx.add_fix(kw.start_offset(), kw.end_offset(), "Avoid using nested modifiers.", |c| {
            if let Some(((a, b), text)) = &fix {
                c.replace(*a, *b, text);
            }
        });
        cx.state::<Ignored>().0.push((s, e));
    }
}

struct Modifier<'pr> {
    keyword: Location<'pr>,
    condition: Node<'pr>,
    /// if/unless (parser `if`), as opposed to while/until
    is_if: bool,
}

/// `basic_conditional? && modifier_form?`
fn modifier<'pr>(node: &Node<'pr>) -> Option<Modifier<'pr>> {
    if let Some(n) = node.as_if_node() {
        let keyword = n.if_keyword_loc().filter(|k| k.as_slice() == b"if" && n.end_keyword_loc().is_none())?;
        return Some(Modifier { keyword, condition: n.predicate(), is_if: true });
    }
    if let Some(n) = node.as_unless_node() {
        n.end_keyword_loc().is_none().then_some(())?;
        return Some(Modifier { keyword: n.keyword_loc(), condition: n.predicate(), is_if: true });
    }
    if let Some(n) = node.as_while_node() {
        (n.closing_loc().is_none() && !n.is_begin_modifier()).then_some(())?;
        return Some(Modifier { keyword: n.keyword_loc(), condition: n.predicate(), is_if: false });
    }
    let n = node.as_until_node()?;
    (n.closing_loc().is_none() && !n.is_begin_modifier()).then_some(())?;
    Some(Modifier { keyword: n.keyword_loc(), condition: n.predicate(), is_if: false })
}

/// `low_precedence?`: operands that bind differently next to `&&`/`||`, or do not parse there.
fn low_precedence(n: &Node) -> bool {
    if is_operator_keyword(n) {
        return !is_logical_operator(n);
    }
    if is_ternary(n) || is_assignment(n) {
        return true;
    }
    match n {
        Node::CallNode { .. } => {
            let c = n.as_call_node().unwrap();
            if c.block().is_some_and(|b| b.as_block_node().is_some()) {
                return false;
            }
            if !is_csend(&c) {
                // `[send prefix_not?]`
                return method_name(&c) == "!" && c.message_loc().is_some_and(|m| m.as_slice() == b"not");
            }
            let has_args = c.arguments().is_some() || c.block().is_some();
            has_args && c.opening_loc().is_none() && !is_operator_method(method_name(&c))
        }
        Node::YieldNode { .. } => {
            let y = n.as_yield_node().unwrap();
            y.arguments().is_some() && y.lparen_loc().is_none()
        }
        Node::SuperNode { .. } => {
            let s = n.as_super_node().unwrap();
            let has_args = s.arguments().is_some() || s.block().is_some_and(|b| b.as_block_argument_node().is_some());
            has_args && s.lparen_loc().is_none() && s.block().is_none_or(|b| b.as_block_node().is_none())
        }
        _ => false,
    }
}

/// `operand_source`: a send with arguments gets parenthesized arguments.
fn operand_source(condition: &Node) -> String {
    let send = condition
        .as_call_node()
        .filter(|c| !is_csend(c) && !c.block().is_some_and(|b| b.as_block_node().is_some()));
    match send.filter(|c| !send_args(c).is_empty() && !is_operator_method(method_name(c))) {
        Some(c) => {
            let recv = c.receiver().map_or(String::new(), |r| format!("{}.", node_text(&r)));
            let args: Vec<_> = send_args(&c).iter().map(|a| node_text(a)).collect();
            format!("{recv}{}({})", method_name(&c), args.join(", "))
        }
        None => node_text(condition).to_string(),
    }
}

fn new_expression(inner: &Modifier, outer: &Modifier) -> String {
    let outer_kw = text(&outer.keyword);
    let operator = if outer_kw == "if" { "&&" } else { "||" };
    let mut lh = operand_source(&outer.condition);
    if (kind::of(&outer.condition) == kind::OR_NODE && operator == "&&") || low_precedence(&outer.condition) {
        lh = format!("({lh})");
    }
    let condition = &inner.condition;
    let negated = outer_kw != text(&inner.keyword);
    let mut rh = operand_source(condition);
    let comparison = condition.as_call_node().is_some_and(|c| {
        !c.block().is_some_and(|b| b.as_block_node().is_some())
            && matches!(method_name(&c), "==" | "===" | "!=" | "<=" | ">=" | ">" | "<")
    });
    if (negated && is_operator_keyword(condition))
        || kind::of(condition) == kind::OR_NODE
        || low_precedence(condition)
        || comparison
    {
        rh = format!("({rh})");
    }
    if negated {
        rh = format!("!{rh}");
    }
    format!("{outer_kw} {lh} {operator} {rh}")
}
