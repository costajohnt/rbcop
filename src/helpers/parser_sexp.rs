//! rubocop-ast `Node#to_s` (parser's `to_sexp`) for a Prism subtree: `(type child...)`, a node
//! child on its own line indented two spaces per level, any other child inline as its `inspect`.
//! The tree is the one RuboCop builds (legacy builder shapes: `a[1]` is a `send`, keyword
//! arguments a `hash`, `-> {}` a block on `(send nil :lambda)`, no `procarg0`).
// ponytail: a string or regexp spanning lines is split per source line by re-measuring each line's
// escapes; an escape this does not size (or a node Prism reports as missing) gives `None`.

use crate::ast::{call_args, int_literal_value, is_csend, is_parser_dstr, is_parser_dsym, parameter_nodes, parser_rest};
use crate::helpers::literal_node::{folded_negative, folded_numeric};
use crate::helpers::ruby_string::{float_to_s, inspect, symbol_inspect};
use crate::source::Source;
use ruby_prism::{ConstantId, Node, NodeList};

enum S {
    Atom(String),
    Node(&'static str, Vec<S>),
}

fn atom(s: impl Into<String>) -> S {
    S::Atom(s.into())
}

fn sym(id: ConstantId) -> S {
    atom(symbol_inspect(id.as_slice()))
}

fn n(t: &'static str, kids: Vec<S>) -> S {
    S::Node(t, kids)
}

fn nil() -> S {
    atom("nil")
}

fn str_node(bytes: &[u8]) -> S {
    n("str", vec![atom(inspect(bytes))])
}

fn int_value(i: ruby_prism::Integer) -> Option<i128> {
    let (neg, digits) = i.to_u32_digits();
    let mut v: i128 = 0;
    for d in digits.iter().rev() {
        v = v.checked_mul(1 << 32)?.checked_add(*d as i128)?;
    }
    Some(if neg { -v } else { v })
}

fn gcd(a: i128, b: i128) -> i128 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

/// A Rational literal's reduced numerator and denominator.
fn rational_parts(node: &Node, neg: bool) -> Option<(i128, i128)> {
    let r = node.as_rational_node()?;
    let (mut num, den) = (int_value(r.numerator())?, int_value(r.denominator())?);
    if neg {
        num = -num;
    }
    let g = gcd(num, den).max(1);
    Some((num / g, den / g))
}

/// Rational#inspect without parentheses: `3/2`, `-1/1`.
fn rational(node: &Node, neg: bool) -> Option<String> {
    let (num, den) = rational_parts(node, neg)?;
    Some(format!("{num}/{den}"))
}

/// Integer#to_s of a literal of any size.
fn decimal(i: ruby_prism::Integer) -> String {
    let (neg, digits) = i.to_u32_digits();
    let mut limbs: Vec<u32> = digits.to_vec();
    let mut chunks: Vec<u32> = Vec::new();
    while limbs.iter().any(|&l| l != 0) {
        let mut rem: u64 = 0;
        for l in limbs.iter_mut().rev() {
            let cur = (rem << 32) | *l as u64;
            *l = (cur / 1_000_000_000) as u32;
            rem = cur % 1_000_000_000;
        }
        chunks.push(rem as u32);
    }
    let mut out = String::from(if neg && !chunks.is_empty() { "-" } else { "" });
    match chunks.split_last() {
        None => out.push('0'),
        Some((top, rest)) => {
            out.push_str(&top.to_string());
            rest.iter().rev().for_each(|c| out.push_str(&format!("{c:09}")));
        }
    }
    out
}

/// A numeric value as a `==` key across Integer, Float and Rational: an integral value as the
/// Integer (Integer#== Float is exact), any other as its Float (Rational#== Float compares `to_f`).
/// ponytail: two Rationals with one `to_f` (17+ digit literals) share a key though they differ.
fn eq_number(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 && f.abs() < 1e36 {
        format!("{}", f as i128)
    } else {
        format!("f{}", float_to_s(f))
    }
}

/// How numeric literals print: `inspect` (`to_s`), or as keys for Ruby's `==` or `eql?` + `hash`.
#[derive(Clone, Copy, PartialEq)]
pub enum Numbers {
    Inspect,
    Eq,
    Eql,
}

/// Unescaped byte length of one source line of a string literal body. `interp`: double-quote
/// escapes; otherwise only `\\` and an escaped delimiter (`delims`) collapse. None: an escape this
/// does not size.
fn unescaped_len(line: &[u8], interp: bool, delims: &[u8]) -> Option<usize> {
    let (mut i, mut len) = (0, 0);
    while i < line.len() {
        if line[i] != b'\\' || i + 1 >= line.len() {
            i += 1;
            len += 1;
            continue;
        }
        let c = line[i + 1];
        if !interp {
            if c == b'\\' || delims.contains(&c) {
                len += 1;
            } else {
                len += 2;
            }
            i += 2;
            continue;
        }
        i += 2;
        match c {
            b'\n' => {}
            b'0'..=b'7' => {
                i += line[i..].iter().take(2).take_while(|b| (b'0'..=b'7').contains(b)).count();
                len += 1;
            }
            b'x' => {
                i += line[i..].iter().take(2).take_while(|b| b.is_ascii_hexdigit()).count();
                len += 1;
            }
            b'u' => {
                let hex = |s: &[u8]| u32::from_str_radix(std::str::from_utf8(s).ok()?, 16).ok().and_then(char::from_u32);
                if line.get(i) == Some(&b'{') {
                    let close = i + line[i..].iter().position(|&b| b == b'}')?;
                    for cp in line[i + 1..close].split(|b| b.is_ascii_whitespace()).filter(|s| !s.is_empty()) {
                        len += hex(cp)?.len_utf8();
                    }
                    i = close + 1;
                } else {
                    len += hex(line.get(i..i + 4)?)?.len_utf8();
                    i += 4;
                }
            }
            b'c' | b'C' | b'M' => {
                i = ctrl_meta_end(line, i - 2)?;
                len += 1;
            }
            _ => {
                // any other escaped character is itself (a multi-byte one keeps its bytes)
                let w = match c {
                    0xf0.. => 4,
                    0xe0.. => 3,
                    0xc0.. => 2,
                    _ => 1,
                };
                len += w;
                i += w - 1;
            }
        }
    }
    Some(len)
}

/// Parser's `str` parts of one string-literal body: one per source line. `value` is Prism's
/// unescaped value of `src`; a backslash-newline in an interpolating body does not split.
fn line_parts(src: &[u8], value: &[u8], interp: bool, delims: &[u8]) -> Option<Vec<S>> {
    Some(line_values(src, value, interp, delims)?.iter().map(|v| str_node(v)).collect())
}

fn line_values<'v>(src: &[u8], value: &'v [u8], interp: bool, delims: &[u8]) -> Option<Vec<&'v [u8]>> {
    let body = src.strip_suffix(b"\n").unwrap_or(src);
    if !body.contains(&b'\n') {
        return Some(if value.is_empty() { Vec::new() } else { vec![value] });
    }
    let mut lines: Vec<&[u8]> = Vec::new();
    let mut start = 0;
    for (i, &b) in src.iter().enumerate() {
        let continued = interp && src[..i].iter().rev().take_while(|&&c| c == b'\\').count() % 2 == 1;
        if b == b'\n' && !continued {
            lines.push(&src[start..=i]);
            start = i + 1;
        }
    }
    if start < src.len() {
        lines.push(&src[start..]);
    }
    let mut out = Vec::new();
    let mut at = 0;
    for l in lines {
        // an unescaped body (a regexp, a quoted heredoc) splits as written
        let len = if src == value { l.len() } else { unescaped_len(l, interp, delims)? };
        out.push(value.get(at..at + len)?);
        at += len;
    }
    (at == value.len()).then_some(out)
}

/// End of a `\c` / `\C-` / `\M-` escape starting at `i` (they nest: `\M-\C-x`); one byte.
fn ctrl_meta_end(line: &[u8], i: usize) -> Option<usize> {
    let after = match line.get(i + 1..i + 3)? {
        [b'c', _] => i + 2,
        [b'C' | b'M', b'-'] => i + 3,
        _ => return None,
    };
    match line.get(after)? {
        b'\\' if matches!(line.get(after + 1), Some(b'c' | b'C' | b'M')) => ctrl_meta_end(line, after),
        b'\\' => Some(after + 2),
        _ => Some(after + 1),
    }
}

/// Parser's `str` parts of a regexp body: the source per line, an escaped terminator that is not a
/// regexp metacharacter unescaped, a backslash-newline dropped (Ruby's tokadd_string).
fn regexp_lines(src: &[u8], term: u8) -> Vec<S> {
    let (mut out, mut cur, mut i) = (Vec::new(), Vec::new(), 0);
    while i < src.len() {
        let c = src[i];
        if c == b'\\' && i + 1 < src.len() {
            let next = src[i + 1];
            if next == term && !b"$*+.?^|)]}>".contains(&next) {
                cur.push(next);
            } else if next != b'\n' {
                cur.extend([c, next]);
            }
            i += 2;
            continue;
        }
        cur.push(c);
        i += 1;
        if c == b'\n' {
            out.push(str_node(&std::mem::take(&mut cur)));
        }
    }
    if !cur.is_empty() {
        out.push(str_node(&cur));
    }
    out
}

/// `(regopt :i :m)` from a regexp's closing delimiter and flags.
fn regopt(closing: &[u8]) -> S {
    let mut opts: Vec<u8> = closing.iter().skip(1).copied().collect();
    opts.sort_unstable();
    opts.dedup();
    n("regopt", opts.iter().map(|&o| atom(format!(":{}", o as char))).collect())
}

/// Whether a literal opened by `opening` interpolates, and its delimiters.
fn quoting(opening: &[u8]) -> (bool, Vec<u8>) {
    let interp = !(opening.starts_with(b"'")
        || opening.starts_with(b"%q")
        || opening.starts_with(b"%w")
        || opening.starts_with(b"%i")
        || opening.starts_with(b"%s")
        || opening.starts_with(b":'")
        || (opening.starts_with(b"<<") && opening.contains(&b'\'')));
    let open = if opening.starts_with(b"<<") { None } else { opening.last().copied() };
    let close = match open {
        Some(b'(') => Some(b')'),
        Some(b'[') => Some(b']'),
        Some(b'{') => Some(b'}'),
        Some(b'<') => Some(b'>'),
        o => o,
    };
    (interp, open.into_iter().chain(close).collect())
}

struct Cv<'s> {
    src: &'s Source<'s>,
    /// The tree whose `hash` the parser gem caches (`hash_sexp`).
    stale: bool,
    num: Numbers,
}

/// The parser gem dedents a squiggly heredoc's String in place after the node cached its hash, so
/// that hash is the undedented body's when it has indentation to remove.
pub fn stale_heredoc(opening: &[u8], content: &[u8]) -> bool {
    opening.starts_with(b"<<~") && dedents(content)
}

/// Whether a squiggly heredoc body has indentation to remove: its least indented line that is not
/// blank starts with a space or tab (Ruby ignores whitespace-only lines for the dedent level).
fn dedents(body: &[u8]) -> bool {
    let mut lines = body.split(|&b| b == b'\n').filter(|l| l.iter().any(|b| !b" \t\r".contains(b))).peekable();
    lines.peek().is_some() && lines.all(|l| matches!(l.first(), Some(b' ' | b'\t')))
}

impl Cv<'_> {
    /// A Float child: `0.0` and `-0.0` are `==`, `eql?` and hash alike (rb_dbl_hash).
    fn float(&self, f: f64) -> S {
        atom(float_to_s(if self.num != Numbers::Inspect && f == 0.0 { 0.0 } else { f }))
    }

    /// Complex#inspect of `0+xi`; as a `==` key the imaginary part by value (`1i == 1.0i`), as an
    /// `eql?` key by class and value (Complex#eql? checks each part's class).
    fn imaginary(&self, node: &Node, neg: bool) -> Option<String> {
        let num = node.as_imaginary_node()?.numeric();
        let sign = if neg { -1.0 } else { 1.0 };
        if self.num == Numbers::Eq {
            let v = match &num {
                Node::IntegerNode { .. } => (int_literal_value(&num)? * if neg { -1 } else { 1 }).to_string(),
                Node::FloatNode { .. } => eq_number(num.as_float_node()?.value() * sign),
                Node::RationalNode { .. } => match rational_parts(&num, neg)? {
                    (p, 1) => p.to_string(),
                    (p, q) => eq_number(p as f64 / q as f64),
                },
                _ => return None,
            };
            return Some(format!("(0+{v}i)"));
        }
        let v = match &num {
            Node::IntegerNode { .. } => (int_literal_value(&num)? * if neg { -1 } else { 1 }).to_string(),
            Node::FloatNode { .. } => {
                let f = num.as_float_node()?.value() * sign;
                float_to_s(if self.num == Numbers::Eql && f == 0.0 { 0.0 } else { f })
            }
            Node::RationalNode { .. } => rational(&num, neg)?,
            _ => return None,
        };
        let (sign, abs) = match v.strip_prefix('-') {
            Some(a) => ('-', a),
            None => ('+', v.as_str()),
        };
        Some(if num.as_rational_node().is_some() { format!("(0{sign}({abs})*i)") } else { format!("(0{sign}{abs}i)") })
    }

    fn opt(&self, node: Option<Node>, depth: usize) -> Option<S> {
        match node {
            Some(n) => self.conv(&n, depth),
            None => Some(nil()),
        }
    }

    fn list<'pr>(&self, kids: impl IntoIterator<Item = Node<'pr>>, depth: usize) -> Option<Vec<S>> {
        kids.into_iter().map(|k| self.conv(&k, depth)).collect()
    }

    /// A body: nil, one statement, or a `begin` of several.
    fn body(&self, node: Option<Node>, depth: usize) -> Option<S> {
        let Some(nd) = node else { return Some(nil()) };
        let Some(st) = nd.as_statements_node() else { return self.conv(&nd, depth) };
        let kids: Vec<Node> = st.body().iter().collect();
        match kids.len() {
            0 => Some(nil()),
            1 => self.conv(&kids[0], depth),
            _ => Some(n("begin", self.list(kids, depth)?)),
        }
    }

    fn stmts(&self, s: Option<ruby_prism::StatementsNode>, depth: usize) -> Option<S> {
        self.body(s.map(|s| s.as_node()), depth)
    }

    fn param(&self, p: &Node, depth: usize) -> Option<S> {
        Some(match p {
            Node::RequiredParameterNode { .. } => n("arg", vec![sym(p.as_required_parameter_node()?.name())]),
            Node::OptionalParameterNode { .. } => {
                let o = p.as_optional_parameter_node()?;
                n("optarg", vec![sym(o.name()), self.conv(&o.value(), depth)?])
            }
            Node::RequiredKeywordParameterNode { .. } => n("kwarg", vec![sym(p.as_required_keyword_parameter_node()?.name())]),
            Node::OptionalKeywordParameterNode { .. } => {
                let o = p.as_optional_keyword_parameter_node()?;
                n("kwoptarg", vec![sym(o.name()), self.conv(&o.value(), depth)?])
            }
            Node::RestParameterNode { .. } => n("restarg", p.as_rest_parameter_node()?.name().map(sym).into_iter().collect()),
            Node::KeywordRestParameterNode { .. } => {
                n("kwrestarg", p.as_keyword_rest_parameter_node()?.name().map(sym).into_iter().collect())
            }
            Node::NoKeywordsParameterNode { .. } => n("kwnilarg", Vec::new()),
            Node::BlockParameterNode { .. } => n("blockarg", vec![p.as_block_parameter_node()?.name().map_or_else(nil, sym)]),
            Node::ForwardingParameterNode { .. } => n("forward-arg", Vec::new()),
            Node::MultiTargetNode { .. } => {
                let m = p.as_multi_target_node()?;
                let kids = m.lefts().iter().chain(parser_rest(m.rest())).chain(m.rights().iter());
                n("mlhs", kids.map(|k| self.param(&k, depth)).collect::<Option<_>>()?)
            }
            Node::SplatNode { .. } => {
                let name = p.as_splat_node()?.expression().and_then(|e| e.as_required_parameter_node()).map(|r| sym(r.name()));
                n("restarg", name.into_iter().collect())
            }
            _ => return None,
        })
    }

    fn args(&self, params: Option<ruby_prism::ParametersNode>, locals: Option<NodeList>, depth: usize) -> Option<S> {
        let mut out = Vec::new();
        if let Some(p) = params {
            for k in parameter_nodes(&p) {
                out.push(self.param(&k, depth)?);
            }
        }
        for l in locals.iter().flat_map(|l| l.iter()) {
            out.push(n("shadowarg", vec![sym(l.as_block_local_variable_node()?.name())]));
        }
        Some(n("args", out))
    }

    /// `(block send args body)` / numblock / itblock for a block or lambda's parameters.
    fn block(&self, call: S, params: Option<Node>, body: Option<Node>, depth: usize) -> Option<S> {
        let body = self.body(body, depth)?;
        Some(match params {
            Some(Node::NumberedParametersNode { .. }) => {
                n("numblock", vec![call, atom(params?.as_numbered_parameters_node()?.maximum().to_string()), body])
            }
            Some(Node::ItParametersNode { .. }) => n("itblock", vec![call, atom(":it"), body]),
            Some(p) => {
                let bp = p.as_block_parameters_node()?;
                n("block", vec![call, self.args(bp.parameters(), Some(bp.locals()), depth)?, body])
            }
            None => n("block", vec![call, n("args", Vec::new()), body]),
        })
    }

    /// A call's arguments plus its block pass, and the attached block if any.
    fn call_kids<'pr>(&self, args: Vec<Node<'pr>>, block: Option<Node<'pr>>, depth: usize) -> Option<(Vec<S>, Option<ruby_prism::BlockNode<'pr>>)> {
        let mut kids = self.list(args, depth)?;
        let mut blk = None;
        match block {
            Some(Node::BlockArgumentNode { .. }) => {
                let b = block?.as_block_argument_node()?;
                kids.push(n("block-pass", vec![self.opt(b.expression(), depth)?]));
            }
            Some(b) => blk = Some(b.as_block_node()?),
            None => {}
        }
        Some((kids, blk))
    }

    fn with_block(&self, call: S, blk: Option<ruby_prism::BlockNode>, depth: usize) -> Option<S> {
        match blk {
            Some(b) => self.block(call, b.parameters(), b.body(), depth),
            None => Some(call),
        }
    }

    fn arguments(args: Option<ruby_prism::ArgumentsNode>) -> Vec<Node> {
        args.map(|a| a.arguments().iter().collect()).unwrap_or_default()
    }

    /// An assignment target without its value (masgn / for / rescue / op-asgn left-hand sides).
    fn target(&self, t: &Node, depth: usize) -> Option<S> {
        Some(match t {
            Node::LocalVariableTargetNode { .. } => n("lvasgn", vec![sym(t.as_local_variable_target_node()?.name())]),
            Node::InstanceVariableTargetNode { .. } => n("ivasgn", vec![sym(t.as_instance_variable_target_node()?.name())]),
            Node::ClassVariableTargetNode { .. } => n("cvasgn", vec![sym(t.as_class_variable_target_node()?.name())]),
            Node::GlobalVariableTargetNode { .. } => n("gvasgn", vec![sym(t.as_global_variable_target_node()?.name())]),
            Node::ConstantTargetNode { .. } => n("casgn", vec![nil(), sym(t.as_constant_target_node()?.name())]),
            Node::ConstantPathTargetNode { .. } => {
                let c = t.as_constant_path_target_node()?;
                n("casgn", vec![self.scope(c.parent(), depth)?, sym(c.name()?)])
            }
            Node::CallTargetNode { .. } => {
                let c = t.as_call_target_node()?;
                let ty = if c.is_safe_navigation() { "csend" } else { "send" };
                n(ty, vec![self.conv(&c.receiver(), depth)?, sym(c.name())])
            }
            Node::IndexTargetNode { .. } => {
                let c = t.as_index_target_node()?;
                let (mut kids, _) = self.call_kids(Self::arguments(c.arguments()), c.block().map(|b| b.as_node()), depth)?;
                kids.insert(0, atom(":[]="));
                kids.insert(0, self.conv(&c.receiver(), depth)?);
                n("send", kids)
            }
            Node::MultiTargetNode { .. } => {
                let m = t.as_multi_target_node()?;
                self.mlhs(m.lefts(), m.rest(), m.rights(), depth)?
            }
            Node::SplatNode { .. } => {
                n("splat", t.as_splat_node()?.expression().map(|e| self.target(&e, depth)).into_iter().collect::<Option<_>>()?)
            }
            _ => return None,
        })
    }

    fn mlhs(&self, lefts: NodeList, rest: Option<Node>, rights: NodeList, depth: usize) -> Option<S> {
        let kids = lefts.iter().chain(parser_rest(rest)).chain(rights.iter());
        Some(n("mlhs", kids.map(|k| self.target(&k, depth)).collect::<Option<_>>()?))
    }

    fn cpath_asgn(&self, t: &ruby_prism::ConstantPathNode, depth: usize) -> Option<S> {
        Some(n("casgn", vec![self.scope(t.parent(), depth)?, sym(t.name()?)]))
    }

    fn scope(&self, parent: Option<Node>, depth: usize) -> Option<S> {
        match parent {
            Some(p) => self.conv(&p, depth),
            None => Some(n("cbase", Vec::new())),
        }
    }

    /// `rescue`/`ensure` structure of a begin body (the BeginNode's statements and clauses).
    fn rescue_body(&self, b: &ruby_prism::BeginNode, depth: usize) -> Option<S> {
        let mut inner = self.stmts(b.statements(), depth)?;
        if let Some(mut r) = b.rescue_clause() {
            let mut kids = vec![inner];
            loop {
                let exc: Vec<Node> = r.exceptions().iter().collect();
                let exc = if exc.is_empty() { nil() } else { n("array", self.list(exc, depth)?) };
                let var = match r.reference() {
                    Some(v) => self.target(&v, depth)?,
                    None => nil(),
                };
                kids.push(n("resbody", vec![exc, var, self.stmts(r.statements(), depth)?]));
                match r.subsequent() {
                    Some(next) => r = next,
                    None => break,
                }
            }
            kids.push(match b.else_clause() {
                Some(e) => self.stmts(e.statements(), depth)?,
                None => nil(),
            });
            inner = n("rescue", kids);
        }
        if let Some(e) = b.ensure_clause() {
            inner = n("ensure", vec![inner, self.stmts(e.statements(), depth)?]);
        }
        Some(inner)
    }

    /// A string-literal node as parser sees it: `str`, or `dstr` (`dsym`, `xstr`) of line parts.
    fn string(&self, node: &Node) -> Option<S> {
        let s = node.as_string_node()?;
        let opening = s.opening_loc().map(|o| o.as_slice()).unwrap_or_default();
        let content = s.content_loc().as_slice();
        // ponytail: the body as written stands in for its unescaped, undedented value.
        let value = if self.stale && stale_heredoc(opening, content) { content } else { s.unescaped() };
        if !is_parser_dstr(node) {
            return Some(str_node(value));
        }
        let (interp, delims) = quoting(opening);
        Some(n("dstr", line_parts(content, value, interp, &delims)?))
    }

    /// Children of an interpolated literal: string parts split per line, `#{}` as `begin`.
    fn parts(&self, parts: NodeList, opening: &[u8], depth: usize) -> Option<Vec<S>> {
        let (interp, delims) = quoting(opening);
        let stale = self.stale && opening.starts_with(b"<<~") && {
            let (first, last) = (parts.iter().next(), parts.iter().last());
            first.zip(last).is_some_and(|(f, l)| {
                dedents(&self.src.text.as_bytes()[f.location().start_offset()..l.location().end_offset()])
            })
        };
        let mut out = Vec::new();
        // A squiggly heredoc line ending in a backslash-newline continues into Prism's next part.
        let mut carry: Vec<u8> = Vec::new();
        for p in parts.iter() {
            if !carry.is_empty() && p.as_string_node().is_none_or(|s| s.opening_loc().is_some()) {
                out.push(str_node(&std::mem::take(&mut carry)));
            }
            match &p {
                Node::StringNode { .. } if p.as_string_node()?.opening_loc().is_none() => {
                    let s = p.as_string_node()?;
                    let src = s.content_loc().as_slice();
                    let value = if stale { src } else { s.unescaped() };
                    let mut vals: Vec<Vec<u8>> = line_values(src, value, interp, &delims)?.into_iter().map(<[u8]>::to_vec).collect();
                    if !carry.is_empty() {
                        carry.extend(if vals.is_empty() { Vec::new() } else { vals.remove(0) });
                        vals.insert(0, std::mem::take(&mut carry));
                    }
                    let body = src.strip_suffix(b"\n").unwrap_or_default();
                    if interp && body.iter().rev().take_while(|&&c| c == b'\\').count() % 2 == 1 {
                        carry = vals.pop().unwrap_or_default();
                    }
                    out.extend(vals.iter().map(|v| str_node(v)));
                }
                Node::EmbeddedStatementsNode { .. } => {
                    let st = p.as_embedded_statements_node()?.statements();
                    out.push(n("begin", self.list(st.iter().flat_map(|s| s.body().iter()), depth)?));
                }
                Node::EmbeddedVariableNode { .. } => out.push(self.conv(&p.as_embedded_variable_node()?.variable(), depth)?),
                _ => out.push(self.conv(&p, depth)?),
            }
        }
        if !carry.is_empty() {
            out.push(str_node(&carry));
        }
        Some(out)
    }

    /// Regexp parts (source text, split per line) and `regopt`.
    fn regexp(&self, parts: Vec<Node>, closing: &[u8], depth: usize) -> Option<S> {
        let mut kids = Vec::new();
        for p in parts {
            match &p {
                Node::StringNode { .. } => kids.extend(regexp_lines(p.location().as_slice(), closing[0])),
                Node::EmbeddedStatementsNode { .. } => {
                    let st = p.as_embedded_statements_node()?.statements();
                    kids.push(n("begin", self.list(st.iter().flat_map(|s| s.body().iter()), depth)?));
                }
                Node::EmbeddedVariableNode { .. } => kids.push(self.conv(&p.as_embedded_variable_node()?.variable(), depth)?),
                _ => return None,
            }
        }
        kids.push(regopt(closing));
        Some(n("regexp", kids))
    }

    /// A hash / keyword-hash element.
    fn element(&self, e: &Node, depth: usize) -> Option<S> {
        match e {
            Node::AssocNode { .. } => {
                let a = e.as_assoc_node()?;
                Some(n("pair", vec![self.conv(&a.key(), depth)?, self.conv(&a.value(), depth)?]))
            }
            Node::AssocSplatNode { .. } => match e.as_assoc_splat_node()?.value() {
                Some(v) => Some(n("kwsplat", vec![self.conv(&v, depth)?])),
                None => Some(n("forwarded-kwrestarg", Vec::new())),
            },
            _ => None,
        }
    }

    /// A pattern (`in` / `=>` right-hand side).
    fn pat(&self, p: &Node, depth: usize) -> Option<S> {
        let depth = depth.checked_sub(1)?;
        let wrap = |c: Option<Node>, inner: S| -> Option<S> {
            match c {
                Some(c) => Some(n("const-pattern", vec![self.conv(&c, depth)?, inner])),
                None => Some(inner),
            }
        };
        let rest = |r: &Node| -> Option<S> {
            match r.as_splat_node()?.expression() {
                Some(e) => Some(n("match-rest", vec![self.pat(&e, depth)?])),
                None => Some(n("match-rest", Vec::new())),
            }
        };
        match p {
            Node::LocalVariableTargetNode { .. } => Some(n("match-var", vec![sym(p.as_local_variable_target_node()?.name())])),
            Node::ArrayPatternNode { .. } => {
                let a = p.as_array_pattern_node()?;
                let tail = a.rest().is_some_and(|r| r.as_implicit_rest_node().is_some());
                let mut kids: Vec<S> = a.requireds().iter().map(|k| self.pat(&k, depth)).collect::<Option<_>>()?;
                if let Some(r) = parser_rest(a.rest()) {
                    kids.push(rest(&r)?);
                }
                for k in a.posts().iter() {
                    kids.push(self.pat(&k, depth)?);
                }
                wrap(a.constant(), n(if tail { "array-pattern-with-tail" } else { "array-pattern" }, kids))
            }
            Node::FindPatternNode { .. } => {
                let f = p.as_find_pattern_node()?;
                let mut kids = vec![rest(&f.left().as_node())?];
                for k in f.requireds().iter() {
                    kids.push(self.pat(&k, depth)?);
                }
                kids.push(rest(&f.right())?);
                wrap(f.constant(), n("find-pattern", kids))
            }
            Node::HashPatternNode { .. } => {
                let h = p.as_hash_pattern_node()?;
                let mut kids = Vec::new();
                for e in h.elements().iter() {
                    let a = e.as_assoc_node()?;
                    let v = a.value();
                    if let Some(i) = v.as_implicit_node() {
                        kids.push(self.pat(&i.value(), depth)?);
                    } else {
                        kids.push(n("pair", vec![self.conv(&a.key(), depth)?, self.pat(&v, depth)?]));
                    }
                }
                match h.rest() {
                    Some(Node::NoKeywordsParameterNode { .. }) => kids.push(n("match-nil-pattern", Vec::new())),
                    Some(r) => match r.as_assoc_splat_node()?.value() {
                        Some(v) => kids.push(n("match-rest", vec![self.pat(&v, depth)?])),
                        None => kids.push(n("match-rest", Vec::new())),
                    },
                    None => {}
                }
                wrap(h.constant(), n("hash-pattern", kids))
            }
            Node::CapturePatternNode { .. } => {
                let c = p.as_capture_pattern_node()?;
                Some(n("match-as", vec![self.pat(&c.value(), depth)?, self.pat(&c.target().as_node(), depth)?]))
            }
            Node::AlternationPatternNode { .. } => {
                let a = p.as_alternation_pattern_node()?;
                Some(n("match-alt", vec![self.pat(&a.left(), depth)?, self.pat(&a.right(), depth)?]))
            }
            Node::PinnedVariableNode { .. } => Some(n("pin", vec![self.conv(&p.as_pinned_variable_node()?.variable(), depth)?])),
            Node::PinnedExpressionNode { .. } => {
                Some(n("pin", vec![n("begin", vec![self.conv(&p.as_pinned_expression_node()?.expression(), depth)?])]))
            }
            Node::SplatNode { .. } => rest(p),
            _ => self.conv(p, depth),
        }
    }

    fn conv(&self, node: &Node, depth: usize) -> Option<S> {
        let depth = depth.checked_sub(1)?;
        if let Some(v) = int_literal_value(node) {
            return Some(n("int", vec![atom(v.to_string())]));
        }
        if let Some(l) = folded_numeric(node) {
            let neg = folded_negative(node);
            return match &l {
                Node::FloatNode { .. } => {
                    Some(n("float", vec![self.float(l.as_float_node()?.value() * if neg { -1.0 } else { 1.0 })]))
                }
                Node::RationalNode { .. } => Some(n("rational", vec![atom(format!("({})", rational(&l, neg)?))])),
                Node::ImaginaryNode { .. } => Some(n("complex", vec![atom(self.imaginary(&l, neg)?)])),
                _ => None,
            };
        }
        let leaf = |t: &'static str| Some(n(t, Vec::new()));
        macro_rules! asgn {
            ($as:ident, $t:literal) => {{
                let w = node.$as()?;
                Some(n($t, vec![sym(w.name()), self.conv(&w.value(), depth)?]))
            }};
        }
        macro_rules! opasgn {
            ($as:ident, $t:literal) => {{
                let w = node.$as()?;
                Some(n("op-asgn", vec![n($t, vec![sym(w.name())]), sym(w.binary_operator()), self.conv(&w.value(), depth)?]))
            }};
        }
        macro_rules! logasgn {
            ($as:ident, $op:literal, $t:literal) => {{
                let w = node.$as()?;
                Some(n($op, vec![n($t, vec![sym(w.name())]), self.conv(&w.value(), depth)?]))
            }};
        }
        match node {
            Node::DefNode { .. } => {
                let d = node.as_def_node()?;
                let mut kids = Vec::new();
                let t = match d.receiver() {
                    Some(r) => {
                        // `def (a).b`: parser drops the parentheses
                        let r = match r.as_parentheses_node() {
                            Some(p) => {
                                let b = p.body()?;
                                match b.as_statements_node() {
                                    Some(s) if s.body().len() == 1 => s.body().iter().next()?,
                                    Some(_) => return None,
                                    None => b,
                                }
                            }
                            None => r,
                        };
                        kids.push(self.conv(&r, depth)?);
                        "defs"
                    }
                    None => "def",
                };
                kids.push(sym(d.name()));
                kids.push(self.args(d.parameters(), None, depth)?);
                kids.push(self.body(d.body(), depth)?);
                Some(n(t, kids))
            }
            Node::CallNode { .. } => {
                let c = node.as_call_node()?;
                let recv = c.receiver();
                // `/re/ =~ x` with a literal regexp: parser's match_with_lvasgn
                if c.name().as_slice() == b"=~" && recv.as_ref().is_some_and(|r| r.as_regular_expression_node().is_some()) {
                    let arg = call_args(&c).into_iter().next()?;
                    return Some(n("match-with-lvasgn", vec![self.conv(&recv?, depth)?, self.conv(&arg, depth)?]));
                }
                let (args, blk) = self.call_kids(call_args(&c), c.block(), depth)?;
                let mut kids = vec![self.opt(recv, depth)?, sym(c.name())];
                kids.extend(args);
                let send = n(if is_csend(&c) { "csend" } else { "send" }, kids);
                self.with_block(send, blk, depth)
            }
            Node::MatchWriteNode { .. } => self.conv(&node.as_match_write_node()?.call().as_node(), depth + 1),
            Node::SuperNode { .. } => {
                let s = node.as_super_node()?;
                let (args, blk) = self.call_kids(Self::arguments(s.arguments()), s.block(), depth)?;
                self.with_block(n("super", args), blk, depth)
            }
            Node::ForwardingSuperNode { .. } => {
                let blk = node.as_forwarding_super_node()?.block();
                self.with_block(n("zsuper", Vec::new()), blk, depth)
            }
            Node::YieldNode { .. } => Some(n("yield", self.list(Self::arguments(node.as_yield_node()?.arguments()), depth)?)),
            Node::ReturnNode { .. } => Some(n("return", self.list(Self::arguments(node.as_return_node()?.arguments()), depth)?)),
            Node::BreakNode { .. } => Some(n("break", self.list(Self::arguments(node.as_break_node()?.arguments()), depth)?)),
            Node::NextNode { .. } => Some(n("next", self.list(Self::arguments(node.as_next_node()?.arguments()), depth)?)),
            Node::RedoNode { .. } => leaf("redo"),
            Node::RetryNode { .. } => leaf("retry"),
            Node::LambdaNode { .. } => {
                let l = node.as_lambda_node()?;
                self.block(n("send", vec![nil(), atom(":lambda")]), l.parameters(), l.body(), depth)
            }
            Node::SplatNode { .. } => match node.as_splat_node()?.expression() {
                Some(e) => Some(n("splat", vec![self.conv(&e, depth)?])),
                None => leaf("forwarded-restarg"),
            },
            Node::ForwardingArgumentsNode { .. } => leaf("forwarded-args"),
            Node::KeywordHashNode { .. } => {
                Some(n("hash", node.as_keyword_hash_node()?.elements().iter().map(|e| self.element(&e, depth)).collect::<Option<_>>()?))
            }
            Node::HashNode { .. } => {
                Some(n("hash", node.as_hash_node()?.elements().iter().map(|e| self.element(&e, depth)).collect::<Option<_>>()?))
            }
            Node::ImplicitNode { .. } => self.conv(&node.as_implicit_node()?.value(), depth),
            Node::LocalVariableReadNode { .. } => {
                let name = node.as_local_variable_read_node()?.name();
                // `_1` below 2.7 is a send for the parser gem
                if self.src.numbered_send(node.location().start_offset()) {
                    return Some(n("send", vec![nil(), sym(name)]));
                }
                Some(n("lvar", vec![sym(name)]))
            }
            Node::ItLocalVariableReadNode { .. } => Some(n("lvar", vec![atom(":it")])),
            Node::InstanceVariableReadNode { .. } => Some(n("ivar", vec![sym(node.as_instance_variable_read_node()?.name())])),
            Node::ClassVariableReadNode { .. } => Some(n("cvar", vec![sym(node.as_class_variable_read_node()?.name())])),
            Node::GlobalVariableReadNode { .. } => Some(n("gvar", vec![sym(node.as_global_variable_read_node()?.name())])),
            Node::NumberedReferenceReadNode { .. } => {
                Some(n("nth-ref", vec![atom(node.as_numbered_reference_read_node()?.number().to_string())]))
            }
            Node::BackReferenceReadNode { .. } => Some(n("back-ref", vec![sym(node.as_back_reference_read_node()?.name())])),
            Node::LocalVariableWriteNode { .. } => asgn!(as_local_variable_write_node, "lvasgn"),
            Node::InstanceVariableWriteNode { .. } => asgn!(as_instance_variable_write_node, "ivasgn"),
            Node::ClassVariableWriteNode { .. } => asgn!(as_class_variable_write_node, "cvasgn"),
            Node::GlobalVariableWriteNode { .. } => asgn!(as_global_variable_write_node, "gvasgn"),
            Node::ConstantWriteNode { .. } => {
                let w = node.as_constant_write_node()?;
                Some(n("casgn", vec![nil(), sym(w.name()), self.conv(&w.value(), depth)?]))
            }
            Node::ConstantPathWriteNode { .. } => {
                let w = node.as_constant_path_write_node()?;
                let t = w.target();
                Some(n("casgn", vec![self.scope(t.parent(), depth)?, sym(t.name()?), self.conv(&w.value(), depth)?]))
            }
            Node::LocalVariableOperatorWriteNode { .. } => opasgn!(as_local_variable_operator_write_node, "lvasgn"),
            Node::InstanceVariableOperatorWriteNode { .. } => opasgn!(as_instance_variable_operator_write_node, "ivasgn"),
            Node::ClassVariableOperatorWriteNode { .. } => opasgn!(as_class_variable_operator_write_node, "cvasgn"),
            Node::GlobalVariableOperatorWriteNode { .. } => opasgn!(as_global_variable_operator_write_node, "gvasgn"),
            Node::LocalVariableOrWriteNode { .. } => logasgn!(as_local_variable_or_write_node, "or-asgn", "lvasgn"),
            Node::InstanceVariableOrWriteNode { .. } => logasgn!(as_instance_variable_or_write_node, "or-asgn", "ivasgn"),
            Node::ClassVariableOrWriteNode { .. } => logasgn!(as_class_variable_or_write_node, "or-asgn", "cvasgn"),
            Node::GlobalVariableOrWriteNode { .. } => logasgn!(as_global_variable_or_write_node, "or-asgn", "gvasgn"),
            Node::LocalVariableAndWriteNode { .. } => logasgn!(as_local_variable_and_write_node, "and-asgn", "lvasgn"),
            Node::InstanceVariableAndWriteNode { .. } => logasgn!(as_instance_variable_and_write_node, "and-asgn", "ivasgn"),
            Node::ClassVariableAndWriteNode { .. } => logasgn!(as_class_variable_and_write_node, "and-asgn", "cvasgn"),
            Node::GlobalVariableAndWriteNode { .. } => logasgn!(as_global_variable_and_write_node, "and-asgn", "gvasgn"),
            Node::ConstantOperatorWriteNode { .. } => {
                let w = node.as_constant_operator_write_node()?;
                Some(n("op-asgn", vec![n("casgn", vec![nil(), sym(w.name())]), sym(w.binary_operator()), self.conv(&w.value(), depth)?]))
            }
            Node::ConstantOrWriteNode { .. } => {
                let w = node.as_constant_or_write_node()?;
                Some(n("or-asgn", vec![n("casgn", vec![nil(), sym(w.name())]), self.conv(&w.value(), depth)?]))
            }
            Node::ConstantAndWriteNode { .. } => {
                let w = node.as_constant_and_write_node()?;
                Some(n("and-asgn", vec![n("casgn", vec![nil(), sym(w.name())]), self.conv(&w.value(), depth)?]))
            }
            Node::ConstantPathOperatorWriteNode { .. } => {
                let w = node.as_constant_path_operator_write_node()?;
                let t = self.cpath_asgn(&w.target(), depth)?;
                Some(n("op-asgn", vec![t, sym(w.binary_operator()), self.conv(&w.value(), depth)?]))
            }
            Node::ConstantPathOrWriteNode { .. } => {
                let w = node.as_constant_path_or_write_node()?;
                Some(n("or-asgn", vec![self.cpath_asgn(&w.target(), depth)?, self.conv(&w.value(), depth)?]))
            }
            Node::ConstantPathAndWriteNode { .. } => {
                let w = node.as_constant_path_and_write_node()?;
                Some(n("and-asgn", vec![self.cpath_asgn(&w.target(), depth)?, self.conv(&w.value(), depth)?]))
            }
            Node::CallOperatorWriteNode { .. } => {
                let w = node.as_call_operator_write_node()?;
                let ty = if w.is_safe_navigation() { "csend" } else { "send" };
                let t = n(ty, vec![self.opt(w.receiver(), depth)?, sym(w.read_name())]);
                Some(n("op-asgn", vec![t, sym(w.binary_operator()), self.conv(&w.value(), depth)?]))
            }
            Node::CallOrWriteNode { .. } => {
                let w = node.as_call_or_write_node()?;
                let ty = if w.is_safe_navigation() { "csend" } else { "send" };
                let t = n(ty, vec![self.opt(w.receiver(), depth)?, sym(w.read_name())]);
                Some(n("or-asgn", vec![t, self.conv(&w.value(), depth)?]))
            }
            Node::CallAndWriteNode { .. } => {
                let w = node.as_call_and_write_node()?;
                let ty = if w.is_safe_navigation() { "csend" } else { "send" };
                let t = n(ty, vec![self.opt(w.receiver(), depth)?, sym(w.read_name())]);
                Some(n("and-asgn", vec![t, self.conv(&w.value(), depth)?]))
            }
            Node::IndexOperatorWriteNode { .. } => {
                let w = node.as_index_operator_write_node()?;
                let t = self.index(w.receiver(), w.arguments(), w.block(), depth)?;
                Some(n("op-asgn", vec![t, sym(w.binary_operator()), self.conv(&w.value(), depth)?]))
            }
            Node::IndexOrWriteNode { .. } => {
                let w = node.as_index_or_write_node()?;
                Some(n("or-asgn", vec![self.index(w.receiver(), w.arguments(), w.block(), depth)?, self.conv(&w.value(), depth)?]))
            }
            Node::IndexAndWriteNode { .. } => {
                let w = node.as_index_and_write_node()?;
                Some(n("and-asgn", vec![self.index(w.receiver(), w.arguments(), w.block(), depth)?, self.conv(&w.value(), depth)?]))
            }
            Node::MultiWriteNode { .. } => {
                let m = node.as_multi_write_node()?;
                Some(n("masgn", vec![self.mlhs(m.lefts(), m.rest(), m.rights(), depth)?, self.conv(&m.value(), depth)?]))
            }
            Node::ConstantReadNode { .. } => Some(n("const", vec![nil(), sym(node.as_constant_read_node()?.name())])),
            Node::ConstantPathNode { .. } => {
                let p = node.as_constant_path_node()?;
                Some(n("const", vec![self.scope(p.parent(), depth)?, sym(p.name()?)]))
            }
            Node::AndNode { .. } => {
                let a = node.as_and_node()?;
                Some(n("and", self.list([a.left(), a.right()], depth)?))
            }
            Node::OrNode { .. } => {
                let o = node.as_or_node()?;
                Some(n("or", self.list([o.left(), o.right()], depth)?))
            }
            Node::IfNode { .. } => {
                let i = node.as_if_node()?;
                let els = match i.subsequent() {
                    Some(e) => match e.as_else_node() {
                        Some(e) => self.stmts(e.statements(), depth)?,
                        None => self.conv(&e, depth)?,
                    },
                    None => nil(),
                };
                Some(n("if", vec![self.conv(&i.predicate(), depth)?, self.stmts(i.statements(), depth)?, els]))
            }
            Node::UnlessNode { .. } => {
                let u = node.as_unless_node()?;
                let els = self.stmts(u.else_clause().and_then(|e| e.statements()), depth)?;
                Some(n("if", vec![self.conv(&u.predicate(), depth)?, els, self.stmts(u.statements(), depth)?]))
            }
            Node::CaseNode { .. } => {
                let c = node.as_case_node()?;
                let mut kids = vec![self.opt(c.predicate(), depth)?];
                for w in c.conditions().iter() {
                    let w = w.as_when_node()?;
                    let mut k = self.list(w.conditions().iter(), depth)?;
                    k.push(self.stmts(w.statements(), depth)?);
                    kids.push(n("when", k));
                }
                kids.push(self.stmts(c.else_clause().and_then(|e| e.statements()), depth)?);
                Some(n("case", kids))
            }
            Node::CaseMatchNode { .. } => {
                let c = node.as_case_match_node()?;
                let mut kids = vec![self.opt(c.predicate(), depth)?];
                for i in c.conditions().iter() {
                    let i = i.as_in_node()?;
                    let (pat, guard) = match i.pattern() {
                        Node::IfNode { .. } => {
                            let g = i.pattern().as_if_node()?;
                            (g.statements()?.body().iter().next()?, n("if-guard", vec![self.conv(&g.predicate(), depth)?]))
                        }
                        Node::UnlessNode { .. } => {
                            let g = i.pattern().as_unless_node()?;
                            (g.statements()?.body().iter().next()?, n("unless-guard", vec![self.conv(&g.predicate(), depth)?]))
                        }
                        p => (p, nil()),
                    };
                    kids.push(n("in-pattern", vec![self.pat(&pat, depth)?, guard, self.stmts(i.statements(), depth)?]));
                }
                kids.push(match c.else_clause() {
                    Some(e) if e.statements().is_none() => n("empty-else", Vec::new()),
                    e => self.stmts(e.and_then(|e| e.statements()), depth)?,
                });
                Some(n("case-match", kids))
            }
            Node::MatchPredicateNode { .. } => {
                let m = node.as_match_predicate_node()?;
                Some(n("match-pattern-p", vec![self.conv(&m.value(), depth)?, self.pat(&m.pattern(), depth)?]))
            }
            Node::MatchRequiredNode { .. } => {
                let m = node.as_match_required_node()?;
                Some(n("match-pattern", vec![self.conv(&m.value(), depth)?, self.pat(&m.pattern(), depth)?]))
            }
            Node::WhileNode { .. } => {
                let w = node.as_while_node()?;
                let t = if w.is_begin_modifier() { "while-post" } else { "while" };
                Some(n(t, vec![self.conv(&w.predicate(), depth)?, self.stmts(w.statements(), depth)?]))
            }
            Node::UntilNode { .. } => {
                let w = node.as_until_node()?;
                let t = if w.is_begin_modifier() { "until-post" } else { "until" };
                Some(n(t, vec![self.conv(&w.predicate(), depth)?, self.stmts(w.statements(), depth)?]))
            }
            Node::ForNode { .. } => {
                let f = node.as_for_node()?;
                Some(n("for", vec![self.target(&f.index(), depth)?, self.conv(&f.collection(), depth)?, self.stmts(f.statements(), depth)?]))
            }
            Node::BeginNode { .. } => {
                let b = node.as_begin_node()?;
                if b.begin_keyword_loc().is_none() {
                    return self.rescue_body(&b, depth);
                }
                if b.rescue_clause().is_none() && b.ensure_clause().is_none() {
                    let kids: Vec<Node> = b.statements().map(|s| s.body().iter().collect()).unwrap_or_default();
                    return Some(n("kwbegin", self.list(kids, depth)?));
                }
                Some(n("kwbegin", vec![self.rescue_body(&b, depth)?]))
            }
            Node::RescueModifierNode { .. } => {
                let r = node.as_rescue_modifier_node()?;
                let resbody = n("resbody", vec![nil(), nil(), self.conv(&r.rescue_expression(), depth)?]);
                Some(n("rescue", vec![self.conv(&r.expression(), depth)?, resbody, nil()]))
            }
            Node::ParenthesesNode { .. } => match node.as_parentheses_node()?.body() {
                None => leaf("begin"),
                Some(b) => match b.as_statements_node() {
                    Some(s) => Some(n("begin", self.list(s.body().iter(), depth)?)),
                    None => Some(n("begin", vec![self.conv(&b, depth)?])),
                },
            },
            Node::ClassNode { .. } => {
                let c = node.as_class_node()?;
                Some(n("class", vec![self.conv(&c.constant_path(), depth)?, self.opt(c.superclass(), depth)?, self.body(c.body(), depth)?]))
            }
            Node::SingletonClassNode { .. } => {
                let c = node.as_singleton_class_node()?;
                Some(n("sclass", vec![self.conv(&c.expression(), depth)?, self.body(c.body(), depth)?]))
            }
            Node::ModuleNode { .. } => {
                let m = node.as_module_node()?;
                Some(n("module", vec![self.conv(&m.constant_path(), depth)?, self.body(m.body(), depth)?]))
            }
            Node::AliasMethodNode { .. } => {
                let a = node.as_alias_method_node()?;
                Some(n("alias", self.list([a.new_name(), a.old_name()], depth)?))
            }
            Node::AliasGlobalVariableNode { .. } => {
                let a = node.as_alias_global_variable_node()?;
                Some(n("alias", self.list([a.new_name(), a.old_name()], depth)?))
            }
            Node::UndefNode { .. } => Some(n("undef", self.list(node.as_undef_node()?.names().iter(), depth)?)),
            Node::DefinedNode { .. } => Some(n("defined?", vec![self.conv(&node.as_defined_node()?.value(), depth)?])),
            Node::PreExecutionNode { .. } => Some(n("preexe", vec![self.stmts(node.as_pre_execution_node()?.statements(), depth)?])),
            Node::PostExecutionNode { .. } => Some(n("postexe", vec![self.stmts(node.as_post_execution_node()?.statements(), depth)?])),
            Node::ShareableConstantNode { .. } => self.conv(&node.as_shareable_constant_node()?.write(), depth + 1),
            Node::SelfNode { .. } => leaf("self"),
            Node::NilNode { .. } => leaf("nil"),
            Node::TrueNode { .. } => leaf("true"),
            Node::FalseNode { .. } => leaf("false"),
            Node::IntegerNode { .. } => Some(n("int", vec![atom(decimal(node.as_integer_node()?.value()))])),
            Node::FloatNode { .. } => Some(n("float", vec![self.float(node.as_float_node()?.value())])),
            Node::RationalNode { .. } => Some(n("rational", vec![atom(format!("({})", rational(node, false)?))])),
            Node::ImaginaryNode { .. } => Some(n("complex", vec![atom(self.imaginary(node, false)?)])),
            Node::SourceFileNode { .. } => Some(str_node(self.src.absolute_path().as_bytes())),
            Node::SourceLineNode { .. } => Some(n("int", vec![atom(self.src.line_of(node.location().start_offset()).to_string())])),
            Node::SourceEncodingNode { .. } => {
                Some(n("const", vec![n("const", vec![nil(), atom(":Encoding")]), atom(":UTF_8")]))
            }
            Node::StringNode { .. } => self.string(node),
            Node::InterpolatedStringNode { .. } => {
                let s = node.as_interpolated_string_node()?;
                let opening = s.opening_loc().map(|o| o.as_slice()).unwrap_or_default();
                Some(n("dstr", self.parts(s.parts(), opening, depth)?))
            }
            Node::XStringNode { .. } => {
                let x = node.as_x_string_node()?;
                let (interp, delims) = quoting(x.opening_loc().as_slice());
                Some(n("xstr", line_parts(x.content_loc().as_slice(), x.unescaped(), interp, &delims)?))
            }
            Node::InterpolatedXStringNode { .. } => {
                let x = node.as_interpolated_x_string_node()?;
                Some(n("xstr", self.parts(x.parts(), x.opening_loc().as_slice(), depth)?))
            }
            Node::SymbolNode { .. } => {
                let s = node.as_symbol_node()?;
                if is_parser_dsym(node, self.src.target_ruby) {
                    let opening = s.opening_loc().map(|o| o.as_slice()).unwrap_or_default();
                    let (interp, delims) = quoting(opening);
                    let src = s.value_loc().map(|v| v.as_slice()).unwrap_or_default();
                    return Some(n("dsym", line_parts(src, s.unescaped(), interp, &delims)?));
                }
                Some(n("sym", vec![atom(symbol_inspect(s.unescaped()))]))
            }
            Node::InterpolatedSymbolNode { .. } => {
                let s = node.as_interpolated_symbol_node()?;
                let opening = s.opening_loc().map(|o| o.as_slice()).unwrap_or_default();
                Some(n("dsym", self.parts(s.parts(), opening, depth)?))
            }
            Node::RegularExpressionNode { .. } => {
                let r = node.as_regular_expression_node()?;
                let closing = r.closing_loc().as_slice();
                let mut kids = regexp_lines(r.content_loc().as_slice(), closing[0]);
                kids.push(regopt(closing));
                Some(n("regexp", kids))
            }
            Node::InterpolatedRegularExpressionNode { .. } => {
                let r = node.as_interpolated_regular_expression_node()?;
                self.regexp(r.parts().iter().collect(), r.closing_loc().as_slice(), depth)
            }
            Node::MatchLastLineNode { .. } => {
                let m = node.as_match_last_line_node()?;
                let closing = m.closing_loc().as_slice();
                let mut kids = regexp_lines(m.content_loc().as_slice(), closing[0]);
                kids.push(regopt(closing));
                Some(n("match-current-line", vec![n("regexp", kids)]))
            }
            Node::InterpolatedMatchLastLineNode { .. } => {
                let m = node.as_interpolated_match_last_line_node()?;
                Some(n("match-current-line", vec![self.regexp(m.parts().iter().collect(), m.closing_loc().as_slice(), depth)?]))
            }
            Node::ArrayNode { .. } => Some(n("array", self.list(node.as_array_node()?.elements().iter(), depth)?)),
            Node::RangeNode { .. } => {
                let r = node.as_range_node()?;
                let t = if r.is_exclude_end() { "erange" } else { "irange" };
                Some(n(t, vec![self.opt(r.left(), depth)?, self.opt(r.right(), depth)?]))
            }
            Node::FlipFlopNode { .. } => {
                let r = node.as_flip_flop_node()?;
                let t = if r.is_exclude_end() { "eflipflop" } else { "iflipflop" };
                Some(n(t, vec![self.opt(r.left(), depth)?, self.opt(r.right(), depth)?]))
            }
            _ => None,
        }
    }

    /// `(send recv :[] args...)` read side of an index operator-assignment.
    fn index(&self, recv: Option<Node>, args: Option<ruby_prism::ArgumentsNode>, block: Option<ruby_prism::BlockArgumentNode>, depth: usize) -> Option<S> {
        let (args, _) = self.call_kids(Self::arguments(args), block.map(|b| b.as_node()), depth)?;
        let mut kids = vec![self.opt(recv, depth)?, atom(":[]")];
        kids.extend(args);
        Some(n("send", kids))
    }
}

fn print(s: &S, indent: usize, out: &mut String) {
    match s {
        S::Atom(a) => {
            out.push(' ');
            out.push_str(a);
        }
        S::Node(t, kids) => {
            out.push_str(&"  ".repeat(indent));
            out.push('(');
            out.push_str(t);
            for k in kids {
                if let S::Node(..) = k {
                    out.push('\n');
                    print(k, indent + 1, out);
                } else {
                    print(k, indent, out);
                }
            }
            out.push(')');
        }
    }
}

/// `node.to_s`, or None when the subtree holds a node this port does not print.
pub fn sexp(node: &Node, src: &Source) -> Option<String> {
    print_tree(node, src, false, Numbers::Inspect)
}

/// `sexp` with numeric literals as `==` (`Numbers::Eq`) or `eql?` (`Numbers::Eql`) keys.
pub fn key_sexp(node: &Node, src: &Source, num: Numbers) -> Option<String> {
    print_tree(node, src, false, num)
}

/// The tree the parser gem's cached `node.hash` describes (what `uniq`, Set and Hash keys compare
/// besides `eql?`), numerics as `eql?` keys: below 3.3 a squiggly heredoc with indentation to
/// remove counts with its undedented body. From 3.3 (Prism's translation) it is `key_sexp`.
pub fn hash_sexp(node: &Node, src: &Source) -> Option<String> {
    print_tree(node, src, src.parser_gem, Numbers::Eql)
}

fn print_tree(node: &Node, src: &Source, stale: bool, num: Numbers) -> Option<String> {
    let cv = Cv { src, stale, num };
    // A StatementsNode is parser's `begin` of its statements (or its one statement).
    let s = match node.as_statements_node() {
        Some(st) => cv.body(Some(st.as_node()), 200)?,
        None => cv.conv(node, 200)?,
    };
    let mut out = String::new();
    print(&s, 0, &mut out);
    Some(out)
}
