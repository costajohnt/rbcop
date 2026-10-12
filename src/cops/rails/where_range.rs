use crate::prelude::*;
use helpers::where_sql::{S, W, find_pair, good_method, template};

pub const NAME: &str = "Rails/WhereRange";
pub static COP: WhereRange = WhereRange;
pub struct WhereRange;

static GTEQ_ANON: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"\A{S}*((?:{W}|\.)+){S}+>={S}+\?{S}*\z")).unwrap());
static LTEQ_ANON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A{S}*((?:{W}|\.)+){S}+(<=?){S}+\?{S}*\z")).unwrap());
// `\1` (a backreference, case-insensitive under /i) is checked by hand.
static RANGE_ANON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\A{S}*((?:{W}|\.)+){S}+>={S}+\?{S}+AND{S}+((?:{W}|\.)+){S}+(<=?){S}+\?{S}*\z")).unwrap()
});
static GTEQ_NAMED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A{S}*((?:{W}|\.)+){S}+>={S}+:({W}+){S}*\z")).unwrap());
static LTEQ_NAMED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"\A{S}*((?:{W}|\.)+){S}+(<=?){S}+:({W}+){S}*\z")).unwrap());
static RANGE_NAMED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)\A{S}*((?:{W}|\.)+){S}+>={S}+:({W}+){S}+AND{S}+((?:{W}|\.)+){S}+(<=?){S}+:({W}+){S}*\z"
    ))
    .unwrap()
});

fn range_operator(op: &str) -> &'static str {
    if op == "<" { "..." } else { ".." }
}

/// parentheses_not_needed?: variable, literal, reference, const, begin, or a call with no
/// arguments or with parentheses.
fn bare(n: &Node) -> bool {
    let variable = matches!(
        n,
        Node::LocalVariableReadNode { .. }
            | Node::InstanceVariableReadNode { .. }
            | Node::ClassVariableReadNode { .. }
            | Node::GlobalVariableReadNode { .. }
            | Node::NumberedReferenceReadNode { .. }
            | Node::BackReferenceReadNode { .. }
            | Node::ConstantReadNode { .. }
            | Node::ConstantPathNode { .. }
            | Node::ParenthesesNode { .. }
    );
    let literal = matches!(
        n,
        Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::XStringNode { .. }
            | Node::InterpolatedXStringNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::RegularExpressionNode { .. }
            | Node::InterpolatedRegularExpressionNode { .. }
            | Node::ArrayNode { .. }
            | Node::HashNode { .. }
            | Node::KeywordHashNode { .. }
            | Node::RangeNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::RationalNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
    );
    let call = n.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()));
    variable || literal || call.is_some_and(|c| send_args(&c).is_empty() || c.opening_loc().is_some_and(|o| o.as_slice() == b"("))
}

fn source(n: &Node) -> String {
    if bare(n) { node_text(n).to_string() } else { format!("({})", node_text(n)) }
}

/// `source` of a pair's value (an omitted value is the bare key name).
fn pair_source(p: &ruby_prism::AssocNode) -> String {
    let t = helpers::rails::pair_value_source(p);
    if bare(&helpers::rails::pair_value(p)) { t.to_string() } else { format!("({t})") }
}

impl Cop for WhereRange {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (2.6, f64::MAX)
    }

    fn min_rails(&self) -> f64 {
        6.0
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        if !matches!(name, "where" | "not") {
            return;
        }
        if name == "not"
            && !call.receiver().and_then(|r| helpers::rails::send_node(&r)).is_some_and(|r| method_name(&r) == "where")
        {
            return;
        }
        let Some((t, values)) = template(&call, true) else { return };
        let ruby27 = cx.config.target_ruby >= 2.7;
        let hash = || values.first().filter(|v| helpers::rails::hash_elements(v).is_some());
        let (column, lhs, op, rhs): (String, Option<String>, Option<&str>, Option<String>) =
            if let Some(c) = GTEQ_ANON.captures(&t) {
                (c[1].to_string(), Some(source(&values[0])), Some(".."), None)
            } else if let Some(c) = LTEQ_ANON.captures(&t) {
                if ruby27 {
                    (c[1].to_string(), None, Some(range_operator(&c[2])), Some(source(&values[0])))
                } else {
                    (c[1].to_string(), None, None, None)
                }
            } else if let Some(c) = RANGE_ANON.captures(&t).filter(|c| c[1].eq_ignore_ascii_case(&c[2])) {
                if values.len() >= 2 {
                    (c[1].to_string(), Some(source(&values[0])), Some(range_operator(&c[3])), Some(source(&values[1])))
                } else {
                    (c[1].to_string(), None, None, None)
                }
            } else if let Some(c) = GTEQ_NAMED.captures(&t) {
                match hash() {
                    Some(h) => {
                        // `pair.value` raises when no key matches.
                        let Some(Some(p)) = find_pair(h, &c[2]) else { return };
                        (c[1].to_string(), Some(pair_source(&p)), Some(".."), None)
                    }
                    None => (c[1].to_string(), None, None, None),
                }
            } else if let Some(c) = LTEQ_NAMED.captures(&t) {
                match hash() {
                    Some(h) => {
                        // RuboCop looks the pair up by match 2, the operator, so this finds a key named `<`/`<=`.
                        let Some(p) = find_pair(h, &c[2]) else { return };
                        match p {
                            Some(p) if ruby27 => (c[1].to_string(), None, Some(range_operator(&c[2])), Some(pair_source(&p))),
                            _ => (c[1].to_string(), None, None, None),
                        }
                    }
                    None => (c[1].to_string(), None, None, None),
                }
            } else if let Some(c) = RANGE_NAMED.captures(&t).filter(|c| c[1].eq_ignore_ascii_case(&c[3])) {
                match hash() {
                    Some(h) => {
                        let Some(p1) = find_pair(h, &c[2]) else { return };
                        let Some(p2) = find_pair(h, &c[5]) else { return };
                        match (p1, p2) {
                            (Some(a), Some(b)) => {
                                (c[1].to_string(), Some(pair_source(&a)), Some(range_operator(&c[4])), Some(pair_source(&b)))
                            }
                            _ => (c[1].to_string(), None, None, None),
                        }
                    }
                    None => (c[1].to_string(), None, None, None),
                }
            } else {
                return;
            };
        if column.matches('.').count() > 1 {
            return;
        }
        let Some(op) = op else { return };
        let value = format!("{}{op}{}", lhs.unwrap_or_default(), rhs.unwrap_or_default());
        let good = good_method(name, &column, &value);
        let s = call.message_loc().unwrap().start_offset();
        let (_, e) = send_range(&call);
        cx.add_fix(s, e, format!("Use `{good}` instead of manually constructing SQL."), |c| c.replace(s, e, &good));
    }
}
