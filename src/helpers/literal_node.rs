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

/// `basic_literal?`: str int float sym true false nil complex rational (a multi-line string is a
/// parser `dstr`, which is not basic).
pub fn basic_literal(node: &Node) -> bool {
    match node {
        Node::StringNode { .. } => !is_parser_dstr(node),
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

/// Structural key: two nodes are parser-equal (`==`) when their keys are equal. Exact for the
/// literal/const trees `recursive_basic_literal?` accepts; other nodes fall back to type + source.
/// ponytail: `__FILE__` keys use the source text, not the buffer name parser substitutes.
pub fn key(node: &Node) -> String {
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
        Node::FloatNode { .. } => {
            let t = node_text(node).replace('_', "");
            format!("float:{}", t.parse::<f64>().map_or(t, |f| format!("{f:?}")))
        }
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
