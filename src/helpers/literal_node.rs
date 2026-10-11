//! rubocop-ast literal predicates (`literal?`, `basic_literal?`, `recursive_basic_literal?`) and a
//! structural key for literal trees (parser-node equality, e.g. `a:` == `:a` == `"a":`).

use crate::ast::{is_parser_dstr, node_text, text};
use ruby_prism::Node;

/// `literal?`: parser LITERALS (str dstr xstr int float sym dsym array hash regexp true irange
/// erange complex rational false nil).
pub fn literal(node: &Node) -> bool {
    basic_literal(node)
        || matches!(
            node,
            // A multi-line string is a parser `dstr`: not basic, but still a literal.
            Node::StringNode { .. }
                | Node::InterpolatedStringNode { .. }
                | Node::XStringNode { .. }
                | Node::InterpolatedXStringNode { .. }
                | Node::InterpolatedSymbolNode { .. }
                | Node::ArrayNode { .. }
                | Node::HashNode { .. }
                | Node::KeywordHashNode { .. }
                | Node::RegularExpressionNode { .. }
                | Node::InterpolatedRegularExpressionNode { .. }
                | Node::RangeNode { .. }
        )
}

thread_local! {
    /// The file being inspected parses with the parser gem (target < 3.3), whose lexer folds a
    /// separated unary plus too (set by runner::analyze).
    static FOLD_PLUS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether `folded_numeric` folds `+ 1` as well as `- 1` (target < 3.3) for this thread's file.
pub fn set_fold_plus(on: bool) {
    FOLD_PLUS.set(on);
}

/// `- 1` (a minus, whitespace, a numeric literal): Prism makes it a `-@` call, the Prism
/// translation RuboCop parses with folds it into one negative literal (`(int -1)`), as the parser
/// gem does; below 3.3 the parser gem folds `+ 1` into `(int 1)` as well (#249). Returns the
/// literal; `folded_negative` tells the sign.
// ponytail: the parser gem (target < 3.3) also folds `- 1.abs`; the Prism translation does not, so
// it stays a call here.
pub fn folded_numeric<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let call = node.as_call_node()?;
    let recv = call.receiver()?;
    let op = call.name().as_slice();
    ((op == b"-@" || (op == b"+@" && FOLD_PLUS.get()))
        && call.call_operator_loc().is_none()
        && call.arguments().is_none()
        && call.block().is_none()
        && call.message_loc().is_some_and(|m| m.end_offset() <= recv.location().start_offset())
        && matches!(
            recv,
            Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. } | Node::ImaginaryNode { .. }
        ))
    .then_some(recv)
}

/// A folded literal is negative (`- 1`, not `+ 1`).
pub fn folded_negative(node: &Node) -> bool {
    node.as_call_node().is_some_and(|c| c.name().as_slice() == b"-@")
}

/// `kind::of` with a folded `- 1` seen as the literal it folds to (parser's `(int -1)`): use it
/// wherever a cop matches numeric literal kinds.
pub fn folded_kind(node: &Node) -> usize {
    folded_numeric(node).map_or_else(|| crate::node_kind::of(node), |l| crate::node_kind::of(&l))
}

/// A float literal's value, a folded `- 1.5` included.
pub fn float_literal_value(node: &Node) -> Option<f64> {
    match folded_numeric(node) {
        Some(l) => l.as_float_node().map(|f| if folded_negative(node) { -f.value() } else { f.value() }),
        None => node.as_float_node().map(|f| f.value()),
    }
}

/// `basic_literal?`: str int float sym true false nil complex rational (a multi-line string is a
/// parser `dstr`, which is not basic).
pub fn basic_literal(node: &Node) -> bool {
    match node {
        Node::StringNode { .. } => !is_parser_dstr(node),
        Node::CallNode { .. } => folded_numeric(node).is_some(),
        _ => matches!(
            node,
            Node::IntegerNode { .. }
                | Node::FloatNode { .. }
                | Node::SymbolNode { .. }
                | Node::TrueNode { .. }
                | Node::FalseNode { .. }
                | Node::NilNode { .. }
                | Node::ImaginaryNode { .. }
                | Node::RationalNode { .. }
                | Node::SourceFileNode { .. }
                | Node::SourceLineNode { .. }
        ),
    }
}

const RECURSIVE_METHODS: &[&str] = &["==", "===", "!=", "<=", ">=", ">", "<", "*", "!", "<=>"];

/// `recursive_basic_literal?`.
pub fn recursive_basic_literal(node: &Node) -> bool {
    match node {
        Node::CallNode { .. } if folded_numeric(node).is_some() => true,
        Node::CallNode { .. } => {
            let call = node.as_call_node().unwrap();
            // A call with a literal block is a parser `block`, a `&.` call a `csend`: not literal.
            if call.block().is_some() || crate::ast::is_csend(&call) {
                return false;
            }
            let name = crate::ast::method_name(&call);
            RECURSIVE_METHODS.contains(&name)
                && call.receiver().is_some_and(|r| recursive_basic_literal(&r))
                && call.arguments().is_none_or(|a| all_lit(a.arguments().iter()))
        }
        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            recursive_basic_literal(&n.left()) && recursive_basic_literal(&n.right())
        }
        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            recursive_basic_literal(&n.left()) && recursive_basic_literal(&n.right())
        }
        Node::StringNode { .. } | Node::XStringNode { .. } | Node::RegularExpressionNode { .. } => true,
        Node::InterpolatedStringNode { .. }
        | Node::InterpolatedXStringNode { .. }
        | Node::InterpolatedSymbolNode { .. }
        | Node::InterpolatedRegularExpressionNode { .. } => all_lit(parts(node).into_iter()),
        Node::EmbeddedStatementsNode { .. } => {
            node.as_embedded_statements_node().unwrap().statements().is_none_or(|s| all_lit(s.body().iter()))
        }
        Node::ArrayNode { .. } => all_lit(node.as_array_node().unwrap().elements().iter()),
        Node::HashNode { .. } => all_lit(node.as_hash_node().unwrap().elements().iter()),
        Node::KeywordHashNode { .. } => all_lit(node.as_keyword_hash_node().unwrap().elements().iter()),
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            recursive_basic_literal(&n.key()) && recursive_basic_literal(&n.value())
        }
        Node::RangeNode { .. } => {
            let n = node.as_range_node().unwrap();
            n.left().is_none_or(|l| recursive_basic_literal(&l)) && n.right().is_none_or(|r| recursive_basic_literal(&r))
        }
        Node::ParenthesesNode { .. } => match node.as_parentheses_node().unwrap().body() {
            None => true,
            Some(b) => match b.as_statements_node() {
                Some(s) => all_lit(s.body().iter()),
                None => recursive_basic_literal(&b),
            },
        },
        _ => basic_literal(node),
    }
}

fn all_lit<'pr>(mut nodes: impl Iterator<Item = Node<'pr>>) -> bool {
    nodes.all(|n| recursive_basic_literal(&n))
}

fn parts<'pr>(node: &Node<'pr>) -> Vec<Node<'pr>> {
    match node {
        Node::InterpolatedStringNode { .. } => node.as_interpolated_string_node().unwrap().parts().iter().collect(),
        Node::InterpolatedXStringNode { .. } => node.as_interpolated_x_string_node().unwrap().parts().iter().collect(),
        Node::InterpolatedSymbolNode { .. } => node.as_interpolated_symbol_node().unwrap().parts().iter().collect(),
        Node::InterpolatedRegularExpressionNode { .. } => {
            node.as_interpolated_regular_expression_node().unwrap().parts().iter().collect()
        }
        _ => Vec::new(),
    }
}

fn bytes(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

fn join<'pr>(tag: &str, nodes: impl Iterator<Item = Node<'pr>>) -> String {
    let keys: Vec<String> = nodes.map(|n| key(&n)).collect();
    format!("{tag}({})", keys.join(","))
}

/// Ruby Integer value of an integer literal's source (`0x1f`, `1_000`, `-0b1`, `017`).
fn int_value(src: &str) -> String {
    let s = src.replace('_', "");
    let (neg, s) = s.strip_prefix('-').map_or((false, s.as_str()), |r| (true, r));
    let s = s.strip_prefix('+').unwrap_or(s);
    let lower = s.to_ascii_lowercase();
    let (radix, digits) = if let Some(d) = lower.strip_prefix("0x") {
        (16, d)
    } else if let Some(d) = lower.strip_prefix("0b") {
        (2, d)
    } else if let Some(d) = lower.strip_prefix("0o") {
        (8, d)
    } else if let Some(d) = lower.strip_prefix("0d") {
        (10, d)
    } else if lower.len() > 1 && lower.starts_with('0') {
        (8, &lower[1..])
    } else {
        (10, lower.as_str())
    };
    match i128::from_str_radix(digits, radix) {
        Ok(v) => format!("{}", if neg { -v } else { v }),
        Err(_) => src.to_string(),
    }
}

fn float_key(src: &str) -> String {
    let t = src.replace('_', "");
    format!("float:{}", t.parse::<f64>().map_or(t, |f| format!("{f:?}")))
}

/// Structural key: two nodes are parser-equal (`==`) when their keys are equal. Exact for the
/// literal/const trees `recursive_basic_literal?` accepts; other nodes fall back to type + source.
/// ponytail: `__FILE__` keys use the source text, not the buffer name parser substitutes.
pub fn key(node: &Node) -> String {
    // A folded `- 1` is the parser's `(int -1)`: the key of the negated literal (`+ 1`'s is 1's).
    if let Some(l) = folded_numeric(node) {
        let neg = format!("{}{}", if folded_negative(node) { "-" } else { "" }, node_text(&l));
        return match l {
            Node::IntegerNode { .. } => format!("int:{}", int_value(&neg)),
            Node::FloatNode { .. } => float_key(&neg),
            _ => format!("{:?}:{neg}", std::mem::discriminant(&l)),
        };
    }
    match node {
        Node::StringNode { .. } => {
            let s = node.as_string_node().unwrap();
            if is_parser_dstr(node) {
                let v = s.unescaped();
                let lines: Vec<String> = v.split_inclusive(|&b| b == b'\n').map(|l| format!("str:{:?}", bytes(l))).collect();
                format!("dstr({})", lines.join(","))
            } else {
                format!("str:{:?}", bytes(s.unescaped()))
            }
        }
        Node::XStringNode { .. } => format!("xstr(str:{:?})", bytes(node.as_x_string_node().unwrap().unescaped())),
        Node::SymbolNode { .. } => format!("sym:{:?}", bytes(node.as_symbol_node().unwrap().unescaped())),
        Node::IntegerNode { .. } => format!("int:{}", int_value(node_text(node))),
        Node::FloatNode { .. } => float_key(node_text(node)),
        Node::TrueNode { .. } => "true".into(),
        Node::FalseNode { .. } => "false".into(),
        Node::NilNode { .. } => "nil".into(),
        Node::SelfNode { .. } => "self".into(),
        Node::SourceEncodingNode { .. } => "Encoding::UTF_8".into(),
        Node::ConstantReadNode { .. } => text(&node.location()).to_string(),
        Node::ConstantPathNode { .. } => {
            let n = node.as_constant_path_node().unwrap();
            let name = n.name_loc();
            match n.parent() {
                None => format!("::{}", text(&name)),
                Some(p) => format!("{}::{}", key(&p), text(&name)),
            }
        }
        Node::RegularExpressionNode { .. } => {
            let r = node.as_regular_expression_node().unwrap();
            format!("regexp(str:{:?},{})", text(&r.content_loc()), regopt(text(&r.closing_loc())))
        }
        Node::InterpolatedRegularExpressionNode { .. } => {
            let r = node.as_interpolated_regular_expression_node().unwrap();
            format!("{},{}", join("regexp", r.parts().iter()), regopt(text(&r.closing_loc())))
        }
        Node::InterpolatedStringNode { .. } => join("dstr", parts(node).into_iter()),
        Node::InterpolatedXStringNode { .. } => join("xstr", parts(node).into_iter()),
        Node::InterpolatedSymbolNode { .. } => join("dsym", parts(node).into_iter()),
        Node::EmbeddedStatementsNode { .. } => {
            let s = node.as_embedded_statements_node().unwrap().statements();
            join("begin", s.into_iter().flat_map(|s| s.body().iter()))
        }
        Node::ArrayNode { .. } => join("array", node.as_array_node().unwrap().elements().iter()),
        Node::HashNode { .. } => join("hash", node.as_hash_node().unwrap().elements().iter()),
        Node::KeywordHashNode { .. } => join("hash", node.as_keyword_hash_node().unwrap().elements().iter()),
        Node::AssocNode { .. } => {
            let n = node.as_assoc_node().unwrap();
            format!("pair({},{})", key(&n.key()), key(&n.value()))
        }
        Node::RangeNode { .. } => {
            let n = node.as_range_node().unwrap();
            let tag = if n.is_exclude_end() { "erange" } else { "irange" };
            join(tag, n.left().into_iter().chain(n.right()))
        }
        Node::AndNode { .. } => {
            let n = node.as_and_node().unwrap();
            join("and", [n.left(), n.right()].into_iter())
        }
        Node::OrNode { .. } => {
            let n = node.as_or_node().unwrap();
            join("or", [n.left(), n.right()].into_iter())
        }
        Node::ParenthesesNode { .. } => match node.as_parentheses_node().unwrap().body() {
            None => "begin()".into(),
            Some(b) => match b.as_statements_node() {
                Some(s) => join("begin", s.body().iter()),
                None => format!("begin({})", key(&b)),
            },
        },
        Node::CallNode { .. } if node.as_call_node().unwrap().block().is_none() => {
            let call = node.as_call_node().unwrap();
            let recv = call.receiver().map_or("nil".into(), |r| key(&r));
            let args: Vec<String> = call.arguments().into_iter().flat_map(|a| a.arguments().iter()).map(|a| key(&a)).collect();
            let tag = if crate::ast::is_csend(&call) { "csend" } else { "send" };
            format!("{tag}({recv},{},{})", crate::ast::method_name(&call), args.join(","))
        }
        _ => format!("{:?}:{}", std::mem::discriminant(node), node_text(node)),
    }
}

fn regopt(closing: &str) -> String {
    let mut opts: Vec<char> = closing.chars().skip(1).collect();
    opts.sort_unstable();
    opts.dedup();
    format!("regopt:{}", opts.into_iter().collect::<String>())
}

/// Parser `loc.expression` of a node: a label symbol (`a:` / `"a":`) excludes its trailing colon.
pub fn expression_range(node: &Node) -> (usize, usize) {
    let loc = node.location();
    let end = match node.as_symbol_node().and_then(|s| s.closing_loc()) {
        Some(c) if c.as_slice().ends_with(b":") && node.as_symbol_node().unwrap().opening_loc().is_none_or(|o| o.as_slice() != b":") => c.end_offset() - 1,
        _ => loc.end_offset(),
    };
    (loc.start_offset(), end)
}

/// Parser node `type` for literal-ish and common expression nodes (for `node.type ==` comparisons).
/// ponytail: rare node kinds fall back to "other"; extend when a cop compares them.
pub fn parser_type(node: &Node) -> &'static str {
    match node {
        Node::StringNode { .. } if is_parser_dstr(node) => "dstr",
        Node::StringNode { .. } | Node::SourceFileNode { .. } => "str",
        Node::InterpolatedStringNode { .. } => "dstr",
        Node::XStringNode { .. } | Node::InterpolatedXStringNode { .. } => "xstr",
        Node::SymbolNode { .. } => "sym",
        Node::InterpolatedSymbolNode { .. } => "dsym",
        Node::IntegerNode { .. } | Node::SourceLineNode { .. } => "int",
        Node::FloatNode { .. } => "float",
        Node::ImaginaryNode { .. } => "complex",
        Node::RationalNode { .. } => "rational",
        Node::TrueNode { .. } => "true",
        Node::FalseNode { .. } => "false",
        Node::NilNode { .. } => "nil",
        Node::SelfNode { .. } => "self",
        Node::ArrayNode { .. } => "array",
        Node::HashNode { .. } | Node::KeywordHashNode { .. } => "hash",
        Node::RangeNode { .. } if node.as_range_node().unwrap().is_exclude_end() => "erange",
        Node::RangeNode { .. } => "irange",
        Node::RegularExpressionNode { .. } | Node::InterpolatedRegularExpressionNode { .. } => "regexp",
        Node::ParenthesesNode { .. } | Node::StatementsNode { .. } => "begin",
        Node::CallNode { .. } if let Some(n) = folded_numeric(node) => parser_type(&n),
        Node::CallNode { .. } if crate::ast::is_csend(&node.as_call_node().unwrap()) => "csend",
        Node::CallNode { .. } => "send",
        Node::AndNode { .. } => "and",
        Node::OrNode { .. } => "or",
        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => "const",
        Node::LocalVariableReadNode { .. } => "lvar",
        Node::InstanceVariableReadNode { .. } => "ivar",
        Node::ClassVariableReadNode { .. } => "cvar",
        Node::GlobalVariableReadNode { .. } => "gvar",
        Node::WhileNode { .. } if node.as_while_node().unwrap().is_begin_modifier() => "while_post",
        Node::WhileNode { .. } => "while",
        Node::UntilNode { .. } if node.as_until_node().unwrap().is_begin_modifier() => "until_post",
        Node::UntilNode { .. } => "until",
        Node::ForNode { .. } => "for",
        Node::DefinedNode { .. } => "defined?",
        Node::YieldNode { .. } => "yield",
        Node::SuperNode { .. } => "super",
        Node::ForwardingSuperNode { .. } => "zsuper",
        Node::DefNode { .. } if node.as_def_node().unwrap().receiver().is_some() => "defs",
        Node::DefNode { .. } => "def",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_compare_like_parser_nodes() {
        assert_eq!(int_value("0x1_0"), "16");
        assert_eq!(int_value("-017"), "-15");
        let result = ruby_prism::parse(b"{a: 1, :a => 2, \"a\": 3, 16 => 4, 0x10 => 5, \"\\xAD\" => 6, \"\\xCF\" => 7}");
        let program = result.node();
        let hash = program.as_program_node().unwrap().statements().body().iter().next().unwrap();
        let keys: Vec<String> = hash.as_hash_node().unwrap().elements().iter().map(|e| key(&e.as_assoc_node().unwrap().key())).collect();
        assert_eq!(keys[0], keys[1]);
        assert_eq!(keys[1], keys[2]);
        assert_eq!(keys[3], keys[4]);
        assert_ne!(keys[5], keys[6]);
    }
}

/// Whether this thread's file parses with the parser gem (target < 3.3), for helpers that get no
/// Source (set by runner::analyze with `set_fold_plus`).
pub fn parser_gem_file() -> bool {
    FOLD_PLUS.get()
}
