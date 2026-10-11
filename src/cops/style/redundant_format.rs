use crate::helpers::format_string::{format_sequences, sequence_parts};
use crate::helpers::parser_parent::is_top_const;
use crate::helpers::ruby_string::float_to_s;
use crate::prelude::*;
use ruby_prism::{AssocNode, InterpolatedStringNode, InterpolatedSymbolNode, StringNode, Visit};

pub const NAME: &str = "Style/RedundantFormat";
pub static COP: RedundantFormat = RedundantFormat;
pub struct RedundantFormat;

static DIGIT_DOLLAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([0-9]+)\$").unwrap());

impl Cop for RedundantFormat {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let method = method_name(&call);
        // RESTRICT_ON_SEND without an on_csend alias.
        if !matches!(method, "format" | "sprintf") || is_csend(&call) {
            return;
        }
        let args = send_args(&call);
        let (start, end) = send_range(&call);
        let replacement = if let Some(r) = without_additional_args(&call, &args) {
            r
        } else {
            let Some(r) = unnecessary_fields(&call, &args) else { return };
            r
        };
        let msg = format!("Use `{replacement}` directly instead of `{method}`.");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
    }
}

/// `format_without_additional_args?` and the single-argument offense: the replacement, unless
/// the string has format sequences.
fn without_additional_args(call: &ruby_prism::CallNode, args: &[Node]) -> Option<String> {
    if call.receiver().is_some_and(|r| !is_top_const(&r, "Kernel")) {
        return None;
    }
    let [value] = args else { return None };
    let string = match value {
        Node::StringNode { .. } | Node::SourceFileNode { .. } => {
            value.as_string_node().map_or(Some(String::new()), |s| String::from_utf8(s.unescaped().to_vec()).ok())
        }
        Node::InterpolatedStringNode { .. } => dstr_static_value(&value.as_interpolated_string_node().unwrap()),
        Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. } => Some(String::new()),
        _ => return None,
    };
    // Ruby raises on a string it cannot scan (invalid encoding): no offense at all.
    if !format_sequences(&string?).is_empty() {
        return None;
    }
    Some(escape_control_chars(node_text(value)))
}

/// `node.children.select(&:str_type?).map(&:value).join` for a parser dstr.
fn dstr_static_value(node: &InterpolatedStringNode) -> Option<String> {
    let mut out = Vec::new();
    for part in node.parts().iter() {
        let Some(s) = part.as_string_node() else { continue };
        // A concatenated multi-line literal is a nested parser dstr, not a str child.
        if s.opening_loc().is_some() && is_parser_dstr(&part) {
            continue;
        }
        out.extend_from_slice(s.unescaped());
    }
    String::from_utf8(out).ok()
}

/// `string.gsub(/\p{Cc}/) { |s| s.dump[1..-2] }`
fn escape_control_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\x0c' => out.push_str("\\f"),
            '\x0b' => out.push_str("\\v"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x1b' => out.push_str("\\e"),
            '\0'..='\x1f' | '\x7f' => out.push_str(&format!("\\x{:02X}", c as u32)),
            '\u{80}'..='\u{9f}' => out.push_str(&format!("\\u{:04X}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

/// `detect_unnecessary_fields` / `register_all_fields_literal`: the quoted formatted string.
fn unnecessary_fields(call: &ruby_prism::CallNode, args: &[Node]) -> Option<String> {
    let first = args.first()?;
    let s = first.as_string_node()?;
    if is_parser_dstr(first) || is_heredoc(first) {
        return None;
    }
    let string = String::from_utf8(s.unescaped().to_vec()).ok()?;
    let arguments = &args[1..];
    if arguments.is_empty() || args.iter().any(splatted) {
        return None;
    }
    if !all_fields_literal(&string, arguments)? {
        return None;
    }
    let values = arguments.iter().map(argument_value).collect::<Option<Vec<_>>>()?;
    let formatted = sprintf(&string, &values)?;
    // quote: `?a` has no loc.end (Ruby raises there).
    let mut start_delim = text(&s.opening_loc()?).to_string();
    let mut end_delim = text(&s.closing_loc()?).to_string();
    if has_dstr_descendant(call) {
        if start_delim == "'" {
            start_delim = "\"".into();
            end_delim = "\"".into();
        } else if let Some(rest) = start_delim.strip_prefix("%q")
            && let Some(c) = rest.chars().next()
        {
            start_delim = format!("%Q{c}");
        }
    }
    Some(format!("{start_delim}{}{end_delim}", escape_control_chars(&formatted)))
}

/// `splat` or `(hash <kwsplat ...>)`
fn splatted(node: &Node) -> bool {
    match node {
        Node::SplatNode { .. } => true,
        Node::HashNode { .. } => node.as_hash_node().unwrap().elements().iter().any(|e| e.as_assoc_splat_node().is_some()),
        Node::KeywordHashNode { .. } => {
            node.as_keyword_hash_node().unwrap().elements().iter().any(|e| e.as_assoc_splat_node().is_some())
        }
        _ => false,
    }
}

fn is_hash(node: &Node) -> bool {
    matches!(node, Node::HashNode { .. } | Node::KeywordHashNode { .. })
}

/// `node.each_descendant(:dstr, :dsym).any?` over the parser send node.
fn has_dstr_descendant(call: &ruby_prism::CallNode) -> bool {
    struct Finder(bool);
    impl<'pr> Visit<'pr> for Finder {
        fn visit_interpolated_string_node(&mut self, _: &InterpolatedStringNode<'pr>) {
            self.0 = true;
        }
        fn visit_interpolated_symbol_node(&mut self, _: &InterpolatedSymbolNode<'pr>) {
            self.0 = true;
        }
        fn visit_string_node(&mut self, node: &StringNode<'pr>) {
            self.0 |= is_parser_dstr(&node.as_node());
        }
    }
    let mut f = Finder(false);
    if let Some(r) = call.receiver() {
        f.visit(&r);
    }
    if let Some(a) = call.arguments() {
        f.visit(&a.as_node());
    }
    if let Some(b) = call.block().filter(|b| b.as_block_argument_node().is_some()) {
        f.visit(&b);
    }
    f.0
}

// ---------------------------------------------------------------------------------------------
// The check (`all_fields_literal?`)

/// `all_fields_literal?`; None when Ruby raises while checking.
fn all_fields_literal(string: &str, all: &[Node]) -> Option<bool> {
    // `arguments.dup`, as indexes into `all`
    let mut arguments: Vec<usize> = (0..all.len()).collect();
    let sequences = format_sequences(string);
    if sequences.is_empty() {
        return Some(false);
    }
    for seq in &sequences {
        if seq.percent {
            return Some(false);
        }
        let parts = sequence_parts(seq.source);
        let variable_width = parts.width.is_some_and(|w| w.starts_with('*'));
        let vwan = if variable_width {
            let w = parts.width.unwrap();
            let n = if w == "*" { Some(1) } else { DIGIT_DOLLAR.captures(w).and_then(|c| c[1].parse::<usize>().ok()) };
            // unknown_variable_width?
            match n.and_then(|n| positional_argument(&arguments, n)) {
                Some(&a) if numeric(&all[a]) => {}
                _ => return Some(false),
            }
            n
        } else {
            None
        };
        let hash = arguments.iter().find(|&&a| is_hash(&all[a])).copied();
        let found;
        let argument: Option<&Node> = if let (Some(hash), Some(name)) = (hash, seq.name) {
            found = find_hash_value(&all[hash], name);
            found.as_ref()
        } else if let Some(n) = vwan {
            // `delete_at(n - 1)`: index -1 is the last element.
            if n == 0 {
                arguments.pop();
            } else if n - 1 < arguments.len() {
                arguments.remove(n - 1);
            }
            (!arguments.is_empty()).then(|| &all[arguments.remove(0)])
        } else if let Some(c) = DIGIT_DOLLAR.captures_iter(seq.source).last() {
            c[1].parse::<usize>().ok().and_then(|n| positional_argument(&arguments, n)).map(|&a| &all[a])
        } else {
            (!arguments.is_empty()).then(|| &all[arguments.remove(0)])
        };
        let Some(argument) = argument else { return Some(false) };
        let template = seq.name.is_some() && seq.source.contains('{');
        let matching = if template {
            acceptable_literal(argument)
        } else {
            match parts.ty {
                Some("s") => acceptable_literal(argument),
                Some("d" | "i" | "u") => {
                    numeric(argument) && ruby_integer(&argument_value(argument)?).is_some()
                }
                Some("f") => numeric(argument) && ruby_float(&argument_value(argument)?).is_some(),
                _ => false,
            }
        };
        if !matching {
            return Some(false);
        }
        if (parts.width.is_some() || parts.precision.is_some()) && is_parser_dstr(argument) {
            return Some(false);
        }
    }
    Some(true)
}

/// `arguments[number - 1] if number - 1 < arguments.size` (`number == 0` reads the last one).
fn positional_argument(arguments: &[usize], number: usize) -> Option<&usize> {
    if number == 0 { arguments.last() } else { arguments.get(number - 1) }
}

/// `find_hash_value_node(hash, name).first`: `(pair (sym name) $_)` anywhere in the hash.
fn find_hash_value<'pr>(hash: &Node<'pr>, name: &str) -> Option<Node<'pr>> {
    struct Finder<'n, 'pr> {
        name: &'n [u8],
        found: Option<Node<'pr>>,
    }
    impl<'pr> Visit<'pr> for Finder<'_, 'pr> {
        fn visit_assoc_node(&mut self, node: &AssocNode<'pr>) {
            if self.found.is_none() && node.key().as_symbol_node().is_some_and(|s| s.unescaped() == self.name) {
                self.found = Some(node.value());
                return;
            }
            ruby_prism::visit_assoc_node(self, node);
        }
    }
    let mut f = Finder { name: name.as_bytes(), found: None };
    f.visit(hash);
    f.found
}

/// ACCEPTABLE_LITERAL_TYPES: str dstr sym dsym numeric boolean nil.
fn acceptable_literal(node: &Node) -> bool {
    matches!(
        node,
        Node::StringNode { .. }
            | Node::InterpolatedStringNode { .. }
            | Node::SymbolNode { .. }
            | Node::InterpolatedSymbolNode { .. }
            | Node::IntegerNode { .. }
            | Node::FloatNode { .. }
            | Node::RationalNode { .. }
            | Node::ImaginaryNode { .. }
            | Node::TrueNode { .. }
            | Node::FalseNode { .. }
            | Node::NilNode { .. }
    ) || crate::helpers::literal_node::folded_numeric(node).is_some()
}

/// `numeric?`: a numeric literal, a parser `str`, or a rational/complex expression.
fn numeric(node: &Node) -> bool {
    matches!(node, Node::IntegerNode { .. } | Node::FloatNode { .. } | Node::RationalNode { .. } | Node::ImaginaryNode { .. })
        || (node.as_string_node().is_some() && !is_parser_dstr(node))
        || rational_number(node)
        || complex_number(node)
        || crate::helpers::literal_node::folded_numeric(node).is_some()
}

fn single_paren<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    let body = node.as_parentheses_node()?.body()?;
    let Some(s) = body.as_statements_node() else { return Some(body) };
    let v: Vec<Node> = s.body().iter().collect();
    if v.len() == 1 { v.into_iter().next() } else { None }
}

/// `(send int op literal)`
fn int_op(node: &Node, op: &str, literal: fn(&Node) -> bool) -> bool {
    let Some(c) = node.as_call_node() else { return false };
    !is_csend(&c)
        && method_name(&c) == op
        && c.block().is_none()
        && c.receiver().is_some_and(|r| helpers::literal_node::folded_kind(&r) == kind::INTEGER_NODE)
        && matches!(call_args(&c).as_slice(), [a] if literal(a))
}

fn is_rational(n: &Node) -> bool {
    n.as_rational_node().is_some()
}

fn is_complex(n: &Node) -> bool {
    n.as_imaginary_node().is_some()
}

/// `{rational (send int :/ rational) (begin rational) (begin (send int :/ rational))}`
fn rational_number(node: &Node) -> bool {
    let inner = single_paren(node);
    let n = inner.as_ref().unwrap_or(node);
    is_rational(n) || int_op(n, "/", is_rational)
}

/// `{complex (send int :+ complex) (begin complex) (begin (send int :+ complex))}`
fn complex_number(node: &Node) -> bool {
    let inner = single_paren(node);
    let n = inner.as_ref().unwrap_or(node);
    is_complex(n) || int_op(n, "+", is_complex)
}

// ---------------------------------------------------------------------------------------------
// Ruby values

#[derive(Clone, Debug)]
enum Num {
    Int(i128),
    Float(f64),
    /// numerator, denominator (> 0), reduced
    Rat(i128, i128),
}

#[derive(Clone, Debug)]
enum V {
    Nil,
    /// A String (also every value `argument_value` takes from the source).
    Str(String),
    Sym(String),
    Num(Num),
    Complex(Num, Num),
    /// Symbol keys by name; other keys never match a format name.
    Hash(Vec<(Option<String>, V)>),
}

/// `argument_value`; None where Ruby raises.
fn argument_value(node: &Node) -> Option<V> {
    if let Some(p) = node.as_parentheses_node() {
        // `argument.children.first`; an empty `()` has none (Ruby raises on nil).
        let body = p.body()?;
        let first = match body.as_statements_node() {
            Some(s) => s.body().iter().next()?,
            None => body,
        };
        return typed_or_nil(&first);
    }
    typed_or_nil(node)
}

fn typed_or_nil(node: &Node) -> Option<V> {
    if node.as_nil_node().is_some() {
        return Some(V::Nil);
    }
    typed_argument_value(node)
}

fn typed_argument_value(node: &Node) -> Option<V> {
    if let Some(d) = node.as_interpolated_symbol_node() {
        // dsym: `children.first.source`
        let first = d.parts().iter().next()?;
        return Some(V::Str(node_text(&first).to_string()));
    }
    if is_hash(node) {
        let elements: Vec<Node> = match node.as_hash_node() {
            Some(h) => h.elements().iter().collect(),
            None => node.as_keyword_hash_node().unwrap().elements().iter().collect(),
        };
        let mut pairs = Vec::new();
        for e in elements {
            let Some(pair) = e.as_assoc_node() else { continue };
            let key = pair.key();
            let name = match &key {
                Node::SymbolNode { .. } => Some(String::from_utf8(key.as_symbol_node().unwrap().unescaped().to_vec()).ok()?),
                // Keys whose rubocop-ast node has a `value`.
                Node::StringNode { .. }
                | Node::InterpolatedStringNode { .. }
                | Node::IntegerNode { .. }
                | Node::FloatNode { .. }
                | Node::RationalNode { .. }
                | Node::ImaginaryNode { .. } => None,
                _ => return None,
            };
            let value = argument_value(&pair.value())?;
            pairs.retain(|(k, _): &(Option<String>, V)| name.is_none() || *k != name);
            pairs.push((name, value));
        }
        return Some(V::Hash(pairs));
    }
    if let Some(l) = crate::helpers::literal_node::folded_numeric(node) {
        return match l {
            Node::IntegerNode { .. } => int_literal_value(node).map(|i| V::Num(Num::Int(i))),
            Node::FloatNode { .. } => crate::helpers::literal_node::float_literal_value(node).map(|f| V::Num(Num::Float(f))),
            _ => None,
        };
    }
    if rational_number(node) {
        return parse_rational_prefix(node_text(node)).map(|(n, d)| V::Num(Num::Rat(n, d)));
    }
    if complex_number(node) {
        return parse_complex(node_text(node));
    }
    match node {
        Node::StringNode { .. } => Some(V::Str(String::from_utf8(node.as_string_node().unwrap().unescaped().to_vec()).ok()?)),
        Node::InterpolatedStringNode { .. } => Some(V::Str(dstr_value(node)?)),
        Node::SymbolNode { .. } => Some(V::Sym(String::from_utf8(node.as_symbol_node().unwrap().unescaped().to_vec()).ok()?)),
        Node::IntegerNode { .. } => int_literal_value(node).map(|i| V::Num(Num::Int(i))),
        Node::FloatNode { .. } => Some(V::Num(Num::Float(node.as_float_node().unwrap().value()))),
        // ponytail: `__FILE__` is a parser str whose value is the buffer name; give up there.
        Node::SourceFileNode { .. } => None,
        _ => Some(V::Str(node_text(node).to_string())),
    }
}

/// rubocop-ast `DstrNode#value`: str children by value, the others by source.
fn dstr_value(node: &Node) -> Option<String> {
    if let Some(s) = node.as_string_node() {
        return String::from_utf8(s.unescaped().to_vec()).ok();
    }
    let d = node.as_interpolated_string_node()?;
    let mut out = String::new();
    for part in d.parts().iter() {
        match &part {
            Node::StringNode { .. } | Node::InterpolatedStringNode { .. } => out.push_str(&dstr_value(&part)?),
            // `#@a` is a parser ivar child: its source has no `#`.
            Node::EmbeddedVariableNode { .. } => out.push_str(node_text(&part.as_embedded_variable_node().unwrap().variable())),
            _ => out.push_str(node_text(&part)),
        }
    }
    Some(out)
}

fn gcd(a: i128, b: i128) -> i128 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

fn rat(n: i128, d: i128) -> Option<(i128, i128)> {
    if d == 0 {
        return None;
    }
    let g = gcd(n, d).max(1);
    let (n, d) = (n / g, d / g);
    Some(if d < 0 { (n.checked_neg()?, -d) } else { (n, d) })
}

/// `String#to_r` on a literal's source (`5r`, `1.5r`, `3/8r`): the longest rational prefix.
fn parse_rational_prefix(s: &str) -> Option<(i128, i128)> {
    let b = s.as_bytes();
    let mut i = 0;
    let neg = matches!(b.first(), Some(b'-'));
    if matches!(b.first(), Some(b'-' | b'+')) {
        i += 1;
    }
    let digits = |i: &mut usize| {
        let mut v: i128 = 0;
        let mut count = 0u32;
        while *i < b.len() && (b[*i].is_ascii_digit() || (b[*i] == b'_' && count > 0)) {
            if b[*i] != b'_' {
                v = v.checked_mul(10)?.checked_add((b[*i] - b'0') as i128)?;
                count += 1;
            }
            *i += 1;
        }
        Some((v, count))
    };
    let (mut n, _) = digits(&mut i)?;
    let mut d: i128 = 1;
    if i < b.len() && b[i] == b'.' && b.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
        i += 1;
        let (f, count) = digits(&mut i)?;
        let scale = 10i128.checked_pow(count)?;
        n = n.checked_mul(scale)?.checked_add(f)?;
        d = scale;
    }
    if i < b.len() && b[i] == b'/' && b.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
        i += 1;
        let (den, _) = digits(&mut i)?;
        d = d.checked_mul(den)?;
    }
    rat(if neg { -n } else { n }, d)
}

/// `Complex(source)` for `1i`, `1.5i`, `5+0i` (other spellings: give up).
fn parse_complex(s: &str) -> Option<V> {
    let body = s.strip_suffix('i')?;
    let split = body.char_indices().skip(1).filter(|&(_, c)| c == '+' || c == '-').map(|(i, _)| i).last();
    let (re, im) = match split {
        Some(i) if !matches!(body.as_bytes()[i - 1], b'e' | b'E') => (&body[..i], &body[i..]),
        _ => ("0", body),
    };
    Some(V::Complex(parse_real(re)?, parse_real(im)?))
}

fn parse_real(s: &str) -> Option<Num> {
    let t = s.strip_prefix('+').unwrap_or(s);
    if t.is_empty() || t.contains("__") || t.starts_with('_') || t.ends_with('_') {
        return None;
    }
    let clean = t.replace('_', "");
    let digits = clean.strip_prefix('-').unwrap_or(&clean);
    if !digits.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) || digits.is_empty() {
        return None;
    }
    if digits.bytes().all(|c| c.is_ascii_digit()) {
        return clean.parse().ok().map(Num::Int);
    }
    clean.parse().ok().map(Num::Float)
}

/// `Integer(value, exception: false)`
fn ruby_integer(v: &V) -> Option<i128> {
    match v {
        V::Num(n) => num_to_i(n),
        V::Str(s) => parse_ruby_integer(s),
        V::Complex(re, Num::Int(0)) => num_to_i(re),
        _ => None,
    }
}

fn num_to_i(n: &Num) -> Option<i128> {
    match *n {
        Num::Int(i) => Some(i),
        Num::Float(f) if f.is_finite() && f.abs() < 1e38 => Some(f.trunc() as i128),
        Num::Float(_) => None,
        Num::Rat(n, d) => Some(n / d),
    }
}

/// `Float(value, exception: false)`
fn ruby_float(v: &V) -> Option<f64> {
    match v {
        V::Num(n) => Some(num_to_f(n)),
        V::Str(s) => parse_ruby_float(s),
        V::Complex(re, Num::Int(0)) => Some(num_to_f(re)),
        _ => None,
    }
}

fn num_to_f(n: &Num) -> f64 {
    match *n {
        Num::Int(i) => i as f64,
        Num::Float(f) => f,
        Num::Rat(n, d) => n as f64 / d as f64,
    }
}

fn is_ruby_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// Digits with single underscores between them.
fn clean_digits(s: &str, radix: u32) -> Option<String> {
    if s.is_empty() || s.starts_with('_') || s.ends_with('_') || s.contains("__") {
        return None;
    }
    let out: String = s.chars().filter(|&c| c != '_').collect();
    out.chars().all(|c| c.is_digit(radix)).then_some(out)
}

/// `Integer(string)`: sign, `0x`/`0b`/`0o`/`0d`/`0` prefixes, underscores, surrounding blanks.
fn parse_ruby_integer(s: &str) -> Option<i128> {
    let t = s.trim_matches(is_ruby_space);
    let (neg, t) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let lower = t.to_ascii_lowercase();
    let (radix, digits) = if let Some(r) = lower.strip_prefix("0x") {
        (16, r.to_string())
    } else if let Some(r) = lower.strip_prefix("0b") {
        (2, r.to_string())
    } else if let Some(r) = lower.strip_prefix("0o") {
        (8, r.to_string())
    } else if let Some(r) = lower.strip_prefix("0d") {
        (10, r.to_string())
    } else if lower.len() > 1 && lower.starts_with('0') {
        (8, lower.clone())
    } else {
        (10, lower.clone())
    };
    let digits = clean_digits(&digits, radix)?;
    let v = i128::from_str_radix(&digits, radix).ok()?;
    Some(if neg { -v } else { v })
}

/// `Float(string)`: decimal with underscores, `5.`, `.5`, exponent; `0x` integers.
fn parse_ruby_float(s: &str) -> Option<f64> {
    let t = s.trim_matches(is_ruby_space);
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let lower = body.to_ascii_lowercase();
    let v = if let Some(hex) = lower.strip_prefix("0x") {
        // ponytail: hex fractions and `p` exponents are not handled (no offense there).
        i128::from_str_radix(&clean_digits(hex, 16)?, 16).ok()? as f64
    } else {
        let (mantissa, exp) = match lower.split_once('e') {
            Some((m, e)) => {
                let e = e.strip_prefix('+').unwrap_or(e);
                let (eneg, ed) = match e.strip_prefix('-') {
                    Some(r) => (true, r),
                    None => (false, e),
                };
                let ed = clean_digits(ed, 10)?;
                (m, format!("e{}{ed}", if eneg { "-" } else { "" }))
            }
            None => (lower.as_str(), String::new()),
        };
        let (int, frac) = match mantissa.split_once('.') {
            Some((i, f)) => (i, Some(f)),
            None => (mantissa, None),
        };
        let int = if int.is_empty() { String::new() } else { clean_digits(int, 10)? };
        let frac = match frac {
            Some(f) if !f.is_empty() => clean_digits(f, 10)?,
            _ => String::new(),
        };
        if int.is_empty() && frac.is_empty() {
            return None;
        }
        format!("{}.{}{exp}", if int.is_empty() { "0" } else { &int }, if frac.is_empty() { "0" } else { &frac })
            .parse::<f64>()
            .ok()?
    };
    Some(if neg { -v } else { v })
}

// ---------------------------------------------------------------------------------------------
// Kernel#format

/// Deliberate safety difference from RuboCop: the cop evaluates `format` at lint time, so a width,
/// precision or result longer than this many bytes (`format('%999999999d', 1)`) stops the
/// evaluation and the call gets no offense instead of allocating a huge string. A replacement that
/// long would be an unusable line anyway.
const MAX_FORMAT_OUTPUT: usize = 10_000;

const FSHARP: u8 = 1;
const FMINUS: u8 = 2;
const FPLUS: u8 = 4;
const FZERO: u8 = 8;
const FSPACE: u8 = 16;
const FWIDTH: u8 = 32;
const FPREC: u8 = 64;
const FPREC0: u8 = 128;

struct Args<'a> {
    values: &'a [V],
    /// 0 initial, > 0 unnumbered count, -1 numbered, -2 named
    posarg: i64,
    nextarg: usize,
}

impl<'a> Args<'a> {
    fn next(&mut self) -> Option<&'a V> {
        if self.posarg < 0 {
            return None;
        }
        self.posarg += 1;
        self.nextarg += 1;
        self.values.get(self.nextarg - 1)
    }
    fn pos(&mut self, n: usize) -> Option<&'a V> {
        if self.posarg > 0 || self.posarg == -2 || n < 1 {
            return None;
        }
        self.posarg = -1;
        self.values.get(n - 1)
    }
    fn named(&mut self, name: &str) -> Option<&'a V> {
        if self.posarg > 0 || self.posarg == -1 {
            return None;
        }
        self.posarg = -2;
        let [V::Hash(pairs)] = self.values else { return None };
        pairs.iter().rev().find(|(k, _)| k.as_deref() == Some(name)).map(|(_, v)| v)
    }
}

/// `NUM2INT` of a `*` width/precision argument.
fn aster_int(v: &V) -> Option<i64> {
    match v {
        V::Num(n) => num_to_i(n).and_then(|i| i64::try_from(i).ok()),
        _ => None,
    }
}

fn read_digits(b: &[u8], i: &mut usize) -> Option<usize> {
    let mut n: usize = 0;
    while *i < b.len() && b[*i].is_ascii_digit() {
        n = n.checked_mul(10)?.checked_add((b[*i] - b'0') as usize)?;
        *i += 1;
    }
    Some(n)
}

/// Ruby's `format(string, *values)` for the conversions this cop accepts; None where Ruby raises
/// (or for conversions not emulated).
fn sprintf(fmt: &str, values: &[V]) -> Option<String> {
    let b = fmt.as_bytes();
    let mut out = String::new();
    let mut args = Args { values, posarg: 0, nextarg: 0 };
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            let next = fmt[i..].find('%').map_or(b.len(), |p| i + p);
            out.push_str(&fmt[i..next]);
            i = next;
            continue;
        }
        i += 1;
        let mut flags: u8 = 0;
        let mut width: usize = 0;
        let mut prec: usize = 0;
        let mut nextvalue: Option<&V> = None;
        loop {
            let &c = b.get(i)?;
            match c {
                b' ' | b'#' | b'+' | b'-' | b'0' => {
                    if flags & (FWIDTH | FPREC0) != 0 {
                        return None;
                    }
                    flags |= match c {
                        b' ' => FSPACE,
                        b'#' => FSHARP,
                        b'+' => FPLUS,
                        b'-' => FMINUS,
                        _ => FZERO,
                    };
                    i += 1;
                }
                b'1'..=b'9' => {
                    let n = read_digits(b, &mut i)?;
                    if b.get(i) == Some(&b'$') {
                        if nextvalue.is_some() {
                            return None;
                        }
                        nextvalue = Some(args.pos(n)?);
                        i += 1;
                    } else {
                        if flags & (FWIDTH | FPREC0) != 0 {
                            return None;
                        }
                        if n > MAX_FORMAT_OUTPUT {
                            return None;
                        }
                        width = n;
                        flags |= FWIDTH;
                    }
                }
                b'<' | b'{' => {
                    let term = if c == b'<' { b'>' } else { b'}' };
                    let close = b[i..].iter().position(|&x| x == term)? + i;
                    if nextvalue.is_some() {
                        return None;
                    }
                    let v = args.named(&fmt[i + 1..close])?;
                    i = close + 1;
                    if term == b'}' {
                        out.push_str(&format_s(v, flags, width, prec)?);
                        if out.len() > MAX_FORMAT_OUTPUT {
                            return None;
                        }
                        break;
                    }
                    nextvalue = Some(v);
                }
                b'*' => {
                    if flags & (FWIDTH | FPREC0) != 0 {
                        return None;
                    }
                    flags |= FWIDTH;
                    i += 1;
                    let w = aster_int(aster(b, &mut i, &mut args)?)?;
                    if w < 0 {
                        flags |= FMINUS;
                    }
                    width = usize::try_from(w.unsigned_abs()).ok().filter(|&w| w <= MAX_FORMAT_OUTPUT)?;
                }
                b'.' => {
                    if flags & FPREC0 != 0 {
                        return None;
                    }
                    flags |= FPREC | FPREC0;
                    i += 1;
                    if b.get(i) == Some(&b'*') {
                        i += 1;
                        let p = aster_int(aster(b, &mut i, &mut args)?)?;
                        if p < 0 {
                            flags &= !FPREC;
                        }
                        prec = usize::try_from(p.max(0)).ok().filter(|&p| p <= MAX_FORMAT_OUTPUT)?;
                    } else {
                        prec = read_digits(b, &mut i).filter(|&p| p <= MAX_FORMAT_OUTPUT)?;
                    }
                }
                b'\n' | b'\0' | b'%' => {
                    if c == b'%' {
                        i += 1;
                    }
                    if flags != 0 {
                        return None;
                    }
                    out.push('%');
                    break;
                }
                b's' | b'd' | b'i' | b'u' | b'f' => {
                    i += 1;
                    let v = match nextvalue {
                        Some(v) => v,
                        None => args.next()?,
                    };
                    let s = match c {
                        b's' => format_s(v, flags, width, prec)?,
                        b'f' => format_f(v, flags, width, prec)?,
                        _ => format_d(v, flags, width, prec)?,
                    };
                    out.push_str(&s);
                    if out.len() > MAX_FORMAT_OUTPUT {
                        return None;
                    }
                    break;
                }
                // Other conversions (the cop never accepts them) and malformed sequences.
                _ => return None,
            }
        }
    }
    Some(out)
}

/// GETASTER after a `*`: `*N$` or the next argument.
fn aster<'a>(b: &[u8], i: &mut usize, args: &mut Args<'a>) -> Option<&'a V> {
    let start = *i;
    let n = read_digits(b, i)?;
    if *i > start && b.get(*i) == Some(&b'$') {
        *i += 1;
        return args.pos(n);
    }
    *i = start;
    args.next()
}

fn pad(body: String, sign: &str, flags: u8, width: usize, zero_ok: bool) -> String {
    let len = sign.chars().count() + body.chars().count();
    if flags & FWIDTH == 0 || width <= len {
        return format!("{sign}{body}");
    }
    let fill = width - len;
    if flags & FMINUS != 0 {
        format!("{sign}{body}{}", " ".repeat(fill))
    } else if zero_ok && flags & FZERO != 0 {
        format!("{sign}{}{body}", "0".repeat(fill))
    } else {
        format!("{}{sign}{body}", " ".repeat(fill))
    }
}

fn to_s(v: &V) -> Option<String> {
    Some(match v {
        V::Nil => String::new(),
        V::Str(s) | V::Sym(s) => s.clone(),
        V::Num(n) => num_to_s(n),
        V::Complex(re, im) => {
            let neg = match im {
                Num::Int(i) => *i < 0,
                Num::Float(f) => f.is_sign_negative(),
                Num::Rat(..) => return None,
            };
            let abs = match im {
                Num::Int(i) => Num::Int(i.abs()),
                Num::Float(f) => Num::Float(f.abs()),
                Num::Rat(..) => return None,
            };
            if matches!(re, Num::Rat(..)) {
                return None;
            }
            format!("{}{}{}i", num_to_s(re), if neg { '-' } else { '+' }, num_to_s(&abs))
        }
        V::Hash(_) => return None,
    })
}

fn num_to_s(n: &Num) -> String {
    match *n {
        Num::Int(i) => i.to_string(),
        Num::Float(f) => float_to_s(f),
        Num::Rat(n, d) => format!("{n}/{d}"),
    }
}

fn format_s(v: &V, flags: u8, width: usize, prec: usize) -> Option<String> {
    let mut s = to_s(v)?;
    if flags & FPREC != 0 {
        s = s.chars().take(prec).collect();
    }
    Some(pad(s, "", flags, width, false))
}

fn sign_of(neg: bool, flags: u8) -> &'static str {
    if neg {
        "-"
    } else if flags & FPLUS != 0 {
        "+"
    } else if flags & FSPACE != 0 {
        " "
    } else {
        ""
    }
}

fn format_d(v: &V, flags: u8, width: usize, prec: usize) -> Option<String> {
    let n = match v {
        V::Num(n) => num_to_i(n)?,
        V::Str(s) => parse_ruby_integer(s)?,
        V::Complex(re, Num::Int(0)) => num_to_i(re)?,
        _ => return None,
    };
    let mut digits = n.unsigned_abs().to_string();
    if flags & FPREC != 0 {
        if prec == 0 && n == 0 {
            digits.clear();
        }
        if digits.len() < prec {
            digits = format!("{}{digits}", "0".repeat(prec - digits.len()));
        }
    }
    Some(pad(digits, sign_of(n < 0, flags), flags, width, flags & FPREC == 0))
}

fn format_f(v: &V, flags: u8, width: usize, prec: usize) -> Option<String> {
    let prec = if flags & FPREC != 0 { prec } else { 6 };
    // Integers and rationals are formatted exactly.
    let exact = match v {
        V::Num(Num::Int(i)) => Some((*i, 1)),
        V::Num(Num::Rat(n, d)) => Some((*n, *d)),
        _ => None,
    };
    if let Some((n, d)) = exact {
        let num = n.unsigned_abs();
        let (int, frac) = if d == 1 {
            (num.to_string(), "0".repeat(prec))
        } else {
            let d = d as u128;
            let scaled = num.checked_mul(10u128.checked_pow(prec as u32)?)?.checked_add(d / 2)? / d;
            let digits = scaled.to_string();
            if digits.len() > prec {
                (digits[..digits.len() - prec].to_string(), digits[digits.len() - prec..].to_string())
            } else {
                ("0".to_string(), format!("{}{digits}", "0".repeat(prec - digits.len())))
            }
        };
        let body = if prec > 0 { format!("{int}.{frac}") } else { int };
        return Some(pad(body, sign_of(n < 0, flags), flags, width, true));
    }
    let f = match v {
        V::Num(n) => num_to_f(n),
        V::Str(s) => parse_ruby_float(s)?,
        V::Complex(re, Num::Int(0)) => num_to_f(re),
        _ => return None,
    };
    let sign = sign_of(f.is_sign_negative() && !f.is_nan(), flags);
    if !f.is_finite() {
        let body = if f.is_nan() { "NaN" } else { "Inf" };
        return Some(pad(body.to_string(), sign, flags, width, false));
    }
    let mut body = format!("{:.prec$}", f.abs());
    if prec == 0 && flags & FSHARP != 0 {
        body.push('.');
    }
    Some(pad(body, sign, flags, width, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> V {
        V::Str(x.into())
    }

    #[test]
    fn kernel_format() {
        let i = |n| V::Num(Num::Int(n));
        assert_eq!(sprintf("%-+ 05d", &[i(3)]).as_deref(), Some("+3   "));
        assert_eq!(sprintf("%5.3d", &[i(-7)]).as_deref(), Some(" -007"));
        assert_eq!(sprintf("%.0d", &[i(0)]).as_deref(), Some(""));
        assert_eq!(sprintf("$%0*.*f", &[i(5), i(2), V::Num(Num::Float(0.5))]).as_deref(), Some("$00.50"));
        assert_eq!(sprintf("%.0f", &[V::Num(Num::Rat(1, 2))]).as_deref(), Some("1"));
        assert_eq!(sprintf("%010.2f", &[V::Num(Num::Rat(-1, 1))]).as_deref(), Some("-000001.00"));
        assert_eq!(sprintf("%2$s %1$s", &[s("world"), s("hello")]).as_deref(), Some("hello world"));
        assert_eq!(sprintf("%s %1$s", &[s("a")]), None);
        assert_eq!(sprintf("%<a>s", &[s("a")]), None);
        assert_eq!(sprintf("%s %y", &[s("a")]), None);
        assert_eq!(parse_ruby_integer("0_7"), Some(7));
        assert_eq!(parse_ruby_integer("08"), None);
        assert_eq!(parse_ruby_float("5."), Some(5.0));
        assert_eq!(parse_ruby_float("5e"), None);
        assert_eq!(parse_rational_prefix("3/8r"), Some((3, 8)));
    }

    #[test]
    fn huge_width_or_precision_gives_up() {
        let t = std::time::Instant::now();
        assert_eq!(sprintf("%999999999d", &[V::Num(Num::Int(1))]), None);
        assert_eq!(sprintf("%.999999999f", &[V::Num(Num::Float(1.0))]), None);
        assert_eq!(sprintf("%.999999999f", &[V::Num(Num::Rat(1, 3))]), None);
        assert_eq!(sprintf("%*s", &[V::Num(Num::Int(999_999_999)), V::Str("a".into())]), None);
        assert_eq!(sprintf("%10000s%10000s", &[V::Str("a".into()), V::Str("b".into())]), None);
        assert!(t.elapsed() < std::time::Duration::from_secs(1));
    }
}
