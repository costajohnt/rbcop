use crate::helpers::node_types::is_operator_method;
use crate::helpers::parser_parent::{is_operator_keyword, parser_parent};
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

fn new_expression(inner: &Modifier, outer: &Modifier) -> String {
    let outer_kw = text(&outer.keyword);
    let operator = if outer_kw == "if" { "&&" } else { "||" };
    let mut lh = node_text(&outer.condition).to_string();
    if kind::of(&outer.condition) == kind::OR_NODE && operator == "&&" {
        lh = format!("({lh})");
    }
    let condition = &inner.condition;
    let negated = outer_kw != text(&inner.keyword);
    let send = condition
        .as_call_node()
        .filter(|c| !is_csend(c) && !c.block().is_some_and(|b| b.as_block_node().is_some()));
    let mut rh = match send.filter(|c| !send_args(c).is_empty() && !is_operator_method(method_name(c))) {
        Some(c) => {
            let recv = c.receiver().map_or(String::new(), |r| format!("{}.", node_text(&r)));
            let args: Vec<_> = send_args(&c).iter().map(|a| node_text(a)).collect();
            format!("{recv}{}({})", method_name(&c), args.join(", "))
        }
        None => node_text(condition).to_string(),
    };
    let comparison = condition.as_call_node().is_some_and(|c| {
        !c.block().is_some_and(|b| b.as_block_node().is_some())
            && matches!(method_name(&c), "==" | "===" | "!=" | "<=" | ">=" | ">" | "<")
    });
    if (negated && is_operator_keyword(condition)) || kind::of(condition) == kind::OR_NODE || comparison {
        rh = format!("({rh})");
    }
    if negated {
        rh = format!("!{rh}");
    }
    format!("{outer_kw} {lh} {operator} {rh}")
}
