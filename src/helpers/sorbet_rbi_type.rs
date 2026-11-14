//! The parts of the rbi gem (0.5.0) rubocop-sorbet's autocorrections use: `RBI::Type.parse_string`
//! (a Sorbet type expression to an RBI::Type), `Type#to_rbi`, `Type#rbs_string` (RBI's
//! TypePrinter) and `Type#simplify`; plus rubocop-sorbet's `RBSParser` lookups of the RBS
//! comments attached to a node (`rbs_annotation_after`, `rbs_signatures_before`).

use crate::source::Source;
use ruby_prism::Node;

#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Sym(String),
    Str(String),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Type {
    Simple(String),
    Anything,
    AttachedClass,
    Boolean,
    NoReturn,
    SelfType,
    Untyped,
    #[default]
    Void,
    Class(Box<Type>),
    Module(Box<Type>),
    ClassOf(Box<Type>, Vec<Type>),
    Nilable(Box<Type>),
    All(Vec<Type>),
    Any(Vec<Type>),
    Generic(String, Vec<Type>),
    TypeParameter(String),
    TypeAlias(String, Box<Type>),
    Tuple(Vec<Type>),
    Shape(Vec<(Key, Type)>),
    Proc { params: Vec<(String, Type)>, returns: Box<Type>, bind: Option<Box<Type>> },
}

/// `RBI::Type::Error` (parse failures, rescued by the cops) or another RBI::Error / NameError
/// (not rescued: a cop error).
#[derive(Debug, PartialEq)]
pub enum Error {
    Type,
    Other,
}

fn slice<'a>(node: &Node<'a>) -> &'a str {
    crate::ast::node_text(node)
}

fn args<'pr>(call: &ruby_prism::CallNode<'pr>) -> Vec<Node<'pr>> {
    call.arguments().map(|a| a.arguments().iter().collect()).unwrap_or_default()
}

fn exactly<'pr>(call: &ruby_prism::CallNode<'pr>, n: usize) -> Result<Vec<Node<'pr>>, Error> {
    let a = args(call);
    if a.len() == n { Ok(a) } else { Err(Error::Type) }
}

fn at_least<'pr>(call: &ruby_prism::CallNode<'pr>, n: usize) -> Result<Vec<Node<'pr>>, Error> {
    let a = args(call);
    if a.len() >= n { Ok(a) } else { Err(Error::Type) }
}

/// `t?`: `T` or `::T`.
fn is_t(node: Option<&Node>) -> bool {
    match node {
        Some(Node::ConstantReadNode { .. }) => node.unwrap().as_constant_read_node().unwrap().name().as_slice() == b"T",
        Some(Node::ConstantPathNode { .. }) => {
            let p = node.unwrap().as_constant_path_node().unwrap();
            p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"T")
        }
        _ => false,
    }
}

fn path_named(node: &Node, name: &[u8]) -> bool {
    node.as_constant_path_node()
        .is_some_and(|p| is_t(p.parent().as_ref()) && p.name().is_some_and(|n| n.as_slice() == name))
}

fn call_name<'a>(call: &ruby_prism::CallNode<'a>) -> &'a [u8] {
    call.name().as_slice()
}

/// RBI::Type.parse_string.
pub fn parse_string(s: &str) -> Result<Type, Error> {
    let result = ruby_prism::parse(s.as_bytes());
    if result.errors().next().is_some() {
        return Err(Error::Type);
    }
    let program = result.node();
    let stmts: Vec<Node> = program.as_program_node().unwrap().statements().body().iter().collect();
    match stmts.as_slice() {
        [only] => parse_node(only),
        _ => Err(Error::Type),
    }
}

/// RBI::Type.parse_node.
pub fn parse_node(node: &Node) -> Result<Type, Error> {
    match node {
        Node::ConstantReadNode { .. } => Ok(Type::Simple(slice(node).to_string())),
        Node::ConstantPathNode { .. } => {
            Ok(if path_named(node, b"Boolean") { Type::Boolean } else { Type::Simple(slice(node).to_string()) })
        }
        Node::ConstantWriteNode { .. } | Node::ConstantPathWriteNode { .. } => {
            let (value, name) = match node.as_constant_write_node() {
                Some(w) => (w.value(), String::from_utf8_lossy(w.name().as_slice()).into_owned()),
                None => {
                    let w = node.as_constant_path_write_node().unwrap();
                    (w.value(), full_name(&w.target().as_node()))
                }
            };
            if let Some(c) = value.as_call_node()
                && is_t(c.receiver().as_ref())
                && call_name(&c) == b"type_alias"
                && let Some(b) = c.block().and_then(|b| b.as_block_node())
                && let Some(first) = b.body().and_then(|s| s.as_statements_node()).and_then(|s| s.body().iter().next())
            {
                return Ok(Type::TypeAlias(name, Box::new(parse_node(&first)?)));
            }
            Ok(Type::Simple(slice(node).to_string()))
        }
        Node::CallNode { .. } => parse_call(&node.as_call_node().unwrap()),
        Node::ArrayNode { .. } => Ok(Type::Tuple(
            node.as_array_node().unwrap().elements().iter().map(|e| parse_node(&e)).collect::<Result<_, _>>()?,
        )),
        Node::HashNode { .. } | Node::KeywordHashNode { .. } => {
            let elements: Vec<Node> = match node.as_hash_node() {
                Some(h) => h.elements().iter().collect(),
                None => node.as_keyword_hash_node().unwrap().elements().iter().collect(),
            };
            let mut shape: Vec<(Key, Type)> = Vec::new();
            for e in elements {
                let Some(a) = e.as_assoc_node() else { return Err(Error::Type) };
                let key = a.key();
                let key = if let Some(s) = key.as_symbol_node() {
                    Key::Sym(String::from_utf8_lossy(s.unescaped()).into_owned())
                } else if let Some(s) = key.as_string_node() {
                    Key::Str(String::from_utf8_lossy(s.unescaped()).into_owned())
                } else {
                    return Err(Error::Type);
                };
                let value = parse_node(&a.value())?;
                // `.to_h`: a repeated key keeps its first position and its last value.
                match shape.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = value,
                    None => shape.push((key, value)),
                }
            }
            Ok(Type::Shape(shape))
        }
        Node::ParenthesesNode { .. } => {
            let body = node.as_parentheses_node().unwrap().body();
            let Some(s) = body.as_ref().and_then(|b| b.as_statements_node()) else { return Err(Error::Type) };
            let children: Vec<Node> = s.body().iter().collect();
            match children.as_slice() {
                [only] => parse_node(only),
                _ => Err(Error::Type),
            }
        }
        _ => Err(Error::Type),
    }
}

/// Prism ConstantPathNode#full_name (`::` prefix dropped).
fn full_name(node: &Node) -> String {
    slice(node).trim_start_matches("::").to_string()
}

fn parse_call(node: &ruby_prism::CallNode) -> Result<Type, Error> {
    let recv = node.receiver();
    match call_name(node) {
        b"void" => {
            exactly(node, 0)?;
            if recv.is_none() {
                return Ok(Type::Void);
            }
        }
        b"[]" => {
            match &recv {
                Some(r @ Node::ConstantReadNode { .. }) => {
                    let a = at_least(node, 1)?;
                    return Ok(Type::Generic(slice(r).to_string(), a.iter().map(parse_node).collect::<Result<_, _>>()?));
                }
                Some(r @ Node::ConstantPathNode { .. }) => {
                    if path_named(r, b"Class") {
                        let a = exactly(node, 1)?;
                        return Ok(Type::Class(Box::new(parse_node(&a[0])?)));
                    }
                    if path_named(r, b"Module") {
                        let a = exactly(node, 1)?;
                        return Ok(Type::Module(Box::new(parse_node(&a[0])?)));
                    }
                    let a = at_least(node, 1)?;
                    return Ok(Type::Generic(slice(r).to_string(), a.iter().map(parse_node).collect::<Result<_, _>>()?));
                }
                Some(Node::CallNode { .. }) => {
                    let r = recv.as_ref().unwrap().as_call_node().unwrap();
                    if is_t(r.receiver().as_ref()) && call_name(&r) == b"class_of" {
                        let ta = exactly(&r, 1)?;
                        let t = parse_node(&ta[0])?;
                        if !matches!(t, Type::Simple(_)) {
                            return Err(Error::Type);
                        }
                        let a = at_least(node, 1)?;
                        return Ok(Type::ClassOf(Box::new(t), a.iter().map(parse_node).collect::<Result<_, _>>()?));
                    }
                }
                _ => {}
            }
            return Err(Error::Type);
        }
        _ => {}
    }
    if is_t_proc(node) {
        return parse_proc(node);
    }
    if !is_t(recv.as_ref()) {
        return Err(Error::Type);
    }
    let one = |node| -> Result<Type, Error> { parse_node(&exactly(node, 1)?[0]) };
    Ok(match call_name(node) {
        b"nilable" => Type::Nilable(Box::new(one(node)?)),
        b"anything" => exactly(node, 0).map(|_| Type::Anything)?,
        b"untyped" => exactly(node, 0).map(|_| Type::Untyped)?,
        b"noreturn" => exactly(node, 0).map(|_| Type::NoReturn)?,
        b"self_type" => exactly(node, 0).map(|_| Type::SelfType)?,
        b"attached_class" => exactly(node, 0).map(|_| Type::AttachedClass)?,
        b"class_of" => {
            let t = one(node)?;
            if !matches!(t, Type::Simple(_)) {
                return Err(Error::Type);
            }
            Type::ClassOf(Box::new(t), Vec::new())
        }
        b"all" => Type::All(at_least(node, 2)?.iter().map(parse_node).collect::<Result<_, _>>()?),
        b"any" => Type::Any(at_least(node, 2)?.iter().map(parse_node).collect::<Result<_, _>>()?),
        b"type_parameter" => {
            let a = exactly(node, 1)?;
            if a[0].as_symbol_node().is_none() {
                return Err(Error::Type);
            }
            Type::TypeParameter(slice(&a[0]).trim_start_matches(':').to_string())
        }
        _ => return Err(Error::Type),
    })
}

/// `call_chain`: the receivers down to the first non-call, outermost last.
fn call_chain<'pr>(node: &ruby_prism::CallNode<'pr>) -> Vec<Node<'pr>> {
    let mut chain = vec![node.as_node()];
    let mut recv = node.receiver();
    while let Some(r) = recv {
        let next = r.as_call_node().and_then(|c| c.receiver());
        let is_call = r.as_call_node().is_some();
        chain.insert(0, r);
        if !is_call {
            break;
        }
        recv = next;
    }
    chain
}

fn is_t_proc(node: &ruby_prism::CallNode) -> bool {
    let chain = call_chain(node);
    chain.len() >= 2 && is_t(Some(&chain[0])) && chain[1].as_call_node().is_some_and(|c| call_name(&c) == b"proc")
}

fn parse_proc(node: &ruby_prism::CallNode) -> Result<Type, Error> {
    let mut calls = call_chain(node);
    calls.reverse();
    calls.pop();
    let (mut params, mut returns, mut bind) = (Vec::new(), Type::Void, None);
    for call in &calls {
        let Some(call) = call.as_call_node() else { return Err(Error::Type) };
        match call_name(&call) {
            b"params" => {
                let a = args(&call);
                let Some(hash) = a.first().and_then(|h| h.as_keyword_hash_node()) else { return Err(Error::Type) };
                let mut p: Vec<(String, Type)> = Vec::new();
                for e in hash.elements().iter() {
                    let Some(e) = e.as_assoc_node() else { return Err(Error::Type) };
                    let name = slice(&e.key()).trim_end_matches(':').to_string();
                    let t = parse_node(&e.value())?;
                    match p.iter_mut().find(|(k, _)| *k == name) {
                        Some(slot) => slot.1 = t,
                        None => p.push((name, t)),
                    }
                }
                params = p;
            }
            b"returns" => returns = parse_node(&exactly(&call, 1)?[0])?,
            b"void" => returns = Type::Void,
            b"proc" => break,
            b"bind" => bind = Some(Box::new(parse_node(&exactly(&call, 1)?[0])?)),
            _ => return Err(Error::Type),
        }
    }
    Ok(Type::Proc { params, returns: Box::new(returns), bind })
}

/// Ruby Symbol#inspect for a type parameter name.
fn symbol_inspect(name: &str) -> String {
    crate::helpers::ruby_string::symbol_inspect(name.as_bytes())
}

impl Type {
    /// Type#to_rbi.
    pub fn to_rbi(&self) -> String {
        let join = |ts: &[Type]| ts.iter().map(Type::to_rbi).collect::<Vec<_>>().join(", ");
        match self {
            Type::Simple(n) => n.clone(),
            Type::Anything => "::T.anything".into(),
            Type::AttachedClass => "::T.attached_class".into(),
            Type::Boolean => "::T::Boolean".into(),
            Type::NoReturn => "::T.noreturn".into(),
            Type::SelfType => "::T.self_type".into(),
            Type::Untyped => "::T.untyped".into(),
            Type::Void => "void".into(),
            Type::Class(t) => format!("::T::Class[{}]", t.to_rbi()),
            Type::Module(t) => format!("::T::Module[{}]", t.to_rbi()),
            Type::ClassOf(t, p) if p.is_empty() => format!("::T.class_of({})", t.to_rbi()),
            Type::ClassOf(t, p) => format!("::T.class_of({})[{}]", t.to_rbi(), join(p)),
            Type::Nilable(t) => format!("::T.nilable({})", t.to_rbi()),
            Type::All(ts) => format!("::T.all({})", join(ts)),
            Type::Any(ts) => format!("::T.any({})", join(ts)),
            Type::Generic(n, ts) => format!("{n}[{}]", join(ts)),
            Type::TypeParameter(n) => format!("::T.type_parameter({})", symbol_inspect(n)),
            Type::TypeAlias(n, t) => format!("{n} = ::T.type_alias {{ {} }}", t.to_rbi()),
            Type::Tuple(ts) => format!("[{}]", join(ts)),
            Type::Shape(fields) if fields.is_empty() => "{}".into(),
            Type::Shape(fields) => {
                let f: Vec<String> = fields
                    .iter()
                    .map(|(k, t)| match k {
                        Key::Sym(n) => format!("{n}: {}", t.to_rbi()),
                        Key::Str(n) => format!("{} => {}", crate::helpers::ruby_string::inspect(n.as_bytes()), t.to_rbi()),
                    })
                    .collect();
                format!("{{ {} }}", f.join(", "))
            }
            Type::Proc { params, returns, bind } => {
                let mut s = String::from("::T.proc");
                if let Some(b) = bind {
                    s.push_str(&format!(".bind({})", b.to_rbi()));
                }
                if !params.is_empty() {
                    let p: Vec<String> = params.iter().map(|(n, t)| format!("{n}: {}", t.to_rbi())).collect();
                    s.push_str(&format!(".params({})", p.join(", ")));
                }
                match **returns {
                    Type::Void => s.push_str(".void"),
                    ref r => s.push_str(&format!(".returns({})", r.to_rbi())),
                }
                s
            }
        }
    }

    /// Type#rbs_string (RBI's TypePrinter). Err: a TypeAlias, which the printer does not handle.
    pub fn rbs_string(&self) -> Result<String, Error> {
        let join = |ts: &[Type], sep: &str| -> Result<String, Error> {
            Ok(ts.iter().map(Type::rbs_string).collect::<Result<Vec<_>, _>>()?.join(sep))
        };
        Ok(match self {
            Type::Simple(n) => translate_t_type(&n.chars().filter(|c| !c.is_ascii_whitespace()).collect::<String>()),
            Type::Boolean => "bool".into(),
            Type::Generic(n, ts) => format!(
                "{}[{}]",
                translate_t_type(&n.chars().filter(|c| !c.is_ascii_whitespace()).collect::<String>()),
                join(ts, ", ")?
            ),
            Type::Anything => "top".into(),
            Type::Void => "void".into(),
            Type::NoReturn => "bot".into(),
            Type::Untyped => "untyped".into(),
            Type::SelfType => "self".into(),
            Type::AttachedClass => "instance".into(),
            Type::Nilable(t) if matches!(**t, Type::Proc { .. }) => format!("({})?", t.rbs_string()?),
            Type::Nilable(t) => format!("{}?", t.rbs_string()?),
            Type::ClassOf(t, p) if p.is_empty() => format!("singleton({})", t.rbs_string()?),
            Type::ClassOf(t, p) => format!("singleton({})[{}]", t.rbs_string()?, join(p, ", ")?),
            Type::All(ts) => format!("({})", join(ts, " & ")?),
            Type::Any(ts) => format!("({})", join(ts, " | ")?),
            Type::Tuple(ts) => format!("[{}]", join(ts, ", ")?),
            Type::Shape(fields) => {
                let mut f = Vec::new();
                for (k, t) in fields {
                    let key = match k {
                        Key::Str(n) => format!("\"{n}\" => "),
                        Key::Sym(n) if is_plain_key(n) => format!("{n}: "),
                        Key::Sym(n) => format!("\"{n}\": "),
                    };
                    f.push(format!("{key}{}", t.rbs_string()?));
                }
                format!("{{{}}}", f.join(", "))
            }
            Type::Proc { params, returns, bind } => {
                let mut s = String::from("^");
                if !params.is_empty() {
                    let p: Vec<String> =
                        params.iter().map(|(n, t)| Ok(format!("{} {n}", t.rbs_string()?))).collect::<Result<_, Error>>()?;
                    s.push_str(&format!("({}) ", p.join(", ")));
                }
                if let Some(b) = bind {
                    s.push_str(&format!("[self: {}] ", b.rbs_string()?));
                }
                s.push_str(&format!("-> {}", returns.rbs_string()?));
                s
            }
            Type::TypeParameter(n) => n.clone(),
            Type::Class(t) => format!("Class[{}]", t.rbs_string()?),
            Type::Module(t) => format!("Module[{}]", t.rbs_string()?),
            Type::TypeAlias(..) => return Err(Error::Other),
        })
    }

    /// Type#normalize.
    fn normalize(&self) -> Type {
        match self {
            Type::Boolean => Type::Any(vec![Type::Simple("TrueClass".into()), Type::Simple("FalseClass".into())]),
            Type::Nilable(t) => Type::Any(vec![Type::Simple("NilClass".into()), t.normalize()]),
            Type::TypeAlias(n, t) => Type::TypeAlias(n.clone(), Box::new(t.normalize())),
            Type::All(ts) | Type::Any(ts) => {
                let all = matches!(self, Type::All(_));
                let mut flat: Vec<Type> = Vec::new();
                for t in ts {
                    match t.normalize() {
                        Type::All(inner) if all => flat.extend(inner.iter().map(Type::normalize)),
                        Type::Any(inner) if !all => flat.extend(inner.iter().map(Type::normalize)),
                        t => flat.push(t),
                    }
                }
                let flat = uniq(flat);
                if flat.len() == 1 {
                    flat.into_iter().next().unwrap()
                } else if all {
                    Type::All(flat)
                } else {
                    Type::Any(flat)
                }
            }
            t => t.clone(),
        }
    }

    /// Type#simplify.
    pub fn simplify(&self) -> Type {
        match self {
            Type::Nilable(t) => match **t {
                Type::Nilable(_) | Type::Untyped => t.simplify(),
                _ => self.clone(),
            },
            Type::TypeAlias(n, t) => Type::TypeAlias(n.clone(), Box::new(t.simplify())),
            Type::All(_) => match self.normalize() {
                Type::All(ts) => Type::All(ts.iter().map(Type::simplify).collect()),
                t => t.simplify(),
            },
            Type::Any(_) => {
                let Type::Any(ts) = self.normalize() else { return self.normalize().simplify() };
                let mut types: Vec<Type> = ts.iter().map(Type::simplify).collect();
                if types.contains(&Type::Untyped) {
                    return Type::Untyped;
                }
                let named = |t: &Type, n: &str| matches!(t, Type::Simple(s) if s == n);
                if types.iter().any(|t| named(t, "TrueClass")) && types.iter().any(|t| named(t, "FalseClass")) {
                    types.retain(|t| !named(t, "TrueClass") && !named(t, "FalseClass"));
                    types.push(Type::Boolean);
                }
                let mut nilable = false;
                let types: Vec<Type> = types
                    .into_iter()
                    .filter_map(|t| match t {
                        Type::Simple(ref s) if s == "NilClass" => {
                            nilable = true;
                            None
                        }
                        Type::Nilable(inner) => {
                            nilable = true;
                            Some(*inner)
                        }
                        t => Some(t),
                    })
                    .collect();
                let types = uniq(types);
                let ty = if types.len() == 1 { types.into_iter().next().unwrap() } else { Type::Any(types) };
                if nilable { Type::Nilable(Box::new(ty)) } else { ty }
            }
            t => t.clone(),
        }
    }
}

/// Array#uniq with Type#eql? (composites compare order-insensitively).
fn uniq(types: Vec<Type>) -> Vec<Type> {
    let mut out: Vec<Type> = Vec::new();
    for t in types {
        if !out.iter().any(|o| same(o, &t)) {
            out.push(t);
        }
    }
    out
}

fn same(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::All(x), Type::All(y)) | (Type::Any(x), Type::Any(y)) => {
            let key = |v: &[Type]| {
                let mut k: Vec<String> = v.iter().map(Type::to_rbi).collect();
                k.sort();
                k
            };
            key(x) == key(y)
        }
        _ => a == b,
    }
}

fn is_plain_key(n: &str) -> bool {
    let b = n.as_bytes();
    !b.is_empty()
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

fn translate_t_type(name: &str) -> String {
    match name {
        "T::Array" => "Array",
        "T::Enumerable" => "Enumerable",
        "T::Enumerator" => "Enumerator",
        "T::Enumerator::Chain" => "Enumerator::Chain",
        "T::Enumerator::Lazy" => "Enumerator::Lazy",
        "T::Hash" => "Hash",
        "T::Set" => "Set",
        "T::Range" => "Range",
        n => n,
    }
    .to_string()
}

/// `RBI::Type.parse_string(source).rbs_string`, as the cops use it (a TypeAlias is an Other error).
pub fn rbs_of(source: &str) -> Result<String, Error> {
    parse_string(source)?.rbs_string()
}

/// RBSParser.rbs_annotation_after: the `#:` comment trailing the node on its last line (only
/// spaces and tabs between them) and its non-empty annotation, stripped.
pub fn rbs_annotation_after(src: &Source, end: usize) -> Option<(usize, usize, String)> {
    let line = src.line_of(end);
    let comment = src.comments.iter().filter(|c| src.line_of(c.start) == line).find(|c| {
        c.start >= end && src.text[end..c.start].bytes().all(|b| b == b' ' || b == b'\t')
    })?;
    let text = src.slice(comment.start, comment.end);
    let annotation = crate::helpers::ruby_string::strip(text.strip_prefix("#:")?);
    (!annotation.is_empty()).then(|| (comment.start, comment.end, annotation.to_string()))
}


// ---- RBI methods and sigs: RBSPrinter#print_method_sig and Printer#print_sig_as_line ----

/// An RBI method parameter (RBI::Parser::TreeBuilder#parse_params).
#[derive(Clone, Debug, PartialEq)]
pub enum MethodParam {
    Req(String),
    Opt(String),
    Rest(Option<String>),
    Kw(String),
    KwOpt(String),
    KwRest(Option<String>),
    Block(Option<String>),
    NoKw,
    Forwarding,
}

impl MethodParam {
    fn name(&self) -> Option<&str> {
        match self {
            MethodParam::Req(n) | MethodParam::Opt(n) | MethodParam::Kw(n) | MethodParam::KwOpt(n) => Some(n),
            MethodParam::Rest(n) | MethodParam::KwRest(n) | MethodParam::Block(n) => n.as_deref(),
            MethodParam::NoKw | MethodParam::Forwarding => None,
        }
    }
}

/// An RBI::Sig. Parameter and return types are kept as Types (the RBS printer parses strings).
#[derive(Clone, Debug, Default)]
pub struct Sig {
    pub params: Vec<(String, Type)>,
    pub returns: Type,
    pub type_params: Vec<String>,
    pub is_abstract: bool,
    pub is_override: bool,
    pub allow_incompatible: Option<&'static str>,
    pub is_overridable: bool,
    pub is_final: bool,
    pub without_runtime: bool,
    pub checked: Option<String>,
    pub bind: Option<Type>,
}

/// RBSPrinter#print_method_sig_content (positional names off). Err(Other): RBI raises.
pub fn print_method_sig(method: &[MethodParam], sig: &Sig, multiline: bool) -> Result<String, Error> {
    let mut out = String::new();
    if !sig.type_params.is_empty() {
        out.push_str(&format!("[{}] ", sig.type_params.join(", ")));
    }
    let no_kw: Vec<&MethodParam> = method.iter().filter(|p| **p == MethodParam::NoKw).collect();
    let method_params: Vec<&MethodParam> = method.iter().filter(|p| **p != MethodParam::NoKw).collect();
    if no_kw.len() > 1 || sig.params.len() != method_params.len() {
        return Err(Error::Other);
    }
    let mut block = None;
    let mut items: Vec<String> = Vec::new();
    for ((name, ty), m) in sig.params.iter().zip(&method_params) {
        if let MethodParam::Block(_) = m {
            if block.is_some() {
                return Err(Error::Other);
            }
            block = Some(ty);
            continue;
        }
        if let Some(mn) = m.name()
            && mn != name
        {
            return Err(Error::Other);
        }
        let t = ty.rbs_string()?;
        items.push(match m {
            MethodParam::Req(_) => t,
            MethodParam::Opt(_) => format!("?{t}"),
            MethodParam::Rest(_) => format!("*{t}"),
            MethodParam::Kw(_) => format!("{name}: {t}"),
            MethodParam::KwOpt(_) => format!("?{name}: {t}"),
            MethodParam::KwRest(None) => format!("**{t}"),
            MethodParam::KwRest(Some(_)) => format!("**{t} {name}"),
            _ => return Err(Error::Other),
        });
    }
    if !no_kw.is_empty() {
        items.push("**nil".into());
    }
    if !items.is_empty() {
        if multiline {
            out.push_str("(\n");
            let n = items.len();
            for (i, it) in items.iter().enumerate() {
                out.push_str(&format!("  {it}{}\n", if i + 1 < n { "," } else { "" }));
            }
            out.push_str(") ");
        } else {
            out.push_str(&format!("({}) ", items.join(", ")));
        }
    }
    if let Some(bt) = block {
        let (mut nilable, mut bt) = (false, bt);
        if let Type::Nilable(inner) = bt {
            nilable = true;
            bt = inner;
        }
        let rbs = bt.rbs_string()?;
        let mut ts = rbs.strip_prefix('^').unwrap_or(&rbs).to_string();
        let mut print = true;
        match bt {
            Type::Untyped => {
                ts = "(?) -> untyped".into();
                nilable = true;
            }
            Type::Simple(n) if n == "NilClass" => print = false,
            Type::Simple(_) => ts = "(?) -> untyped".into(),
            _ => {}
        }
        if print {
            out.push_str(&if nilable { format!("?{{ {ts} }} ") } else { format!("{{ {ts} }} ") });
        }
    }
    out.push_str(&format!("-> {}", sig.returns.rbs_string()?));
    Ok(out)
}

/// RBI Printer#print_sig_as_line (one line, no newline).
pub fn sig_string(sig: &Sig) -> String {
    let mut s = String::new();
    if sig.without_runtime {
        s.push_str("::T::Sig::WithoutRuntime.");
    }
    s.push_str("sig");
    if sig.is_final {
        s.push_str("(:final)");
    }
    s.push_str(" { ");
    let mut modifiers = Vec::new();
    if sig.is_abstract {
        modifiers.push("abstract".to_string());
    }
    if sig.is_override {
        modifiers.push(match sig.allow_incompatible {
            Some(v) => format!("override(allow_incompatible: {v})"),
            None => "override".into(),
        });
    }
    if sig.is_overridable {
        modifiers.push("overridable".into());
    }
    if let Some(b) = &sig.bind {
        modifiers.push(format!("bind({})", b.to_rbi()));
    }
    if !sig.type_params.is_empty() {
        modifiers.push(format!("type_parameters({})", sig.type_params.iter().map(|t| format!(":{t}")).collect::<Vec<_>>().join(", ")));
    }
    if let Some(c) = &sig.checked {
        modifiers.push(format!("checked(:{c})"));
    }
    for m in modifiers {
        s.push_str(&m);
        s.push('.');
    }
    if !sig.params.is_empty() {
        let p: Vec<String> = sig
            .params
            .iter()
            .map(|(n, t)| {
                let name = if matches!(n.as_str(), "*" | "**" | "&") { format!("\"{n}\"") } else { n.clone() };
                format!("{name}: {}", t.to_rbi())
            })
            .collect();
        s.push_str(&format!("params({}).", p.join(", ")));
    }
    match &sig.returns {
        Type::Void => s.push_str("void"),
        t => s.push_str(&format!("returns({})", t.to_rbi())),
    }
    s.push_str(" }");
    s
}

/// A def's (or accessor's) RBI method parameters. Err(Other): a destructuring parameter, which
/// RBI's TreeBuilder raises on.
pub fn method_params(node: &Node) -> Result<Vec<MethodParam>, Error> {
    let Some(def) = node.as_def_node() else { return Ok(Vec::new()) };
    let Some(p) = def.parameters() else { return Ok(Vec::new()) };
    let name = |c: ruby_prism::ConstantId| String::from_utf8_lossy(c.as_slice()).into_owned();
    let mut out = Vec::new();
    let nodes: Vec<Node> = p
        .requireds()
        .iter()
        .chain(p.optionals().iter())
        .chain(p.rest())
        .chain(p.posts().iter())
        .chain(p.keywords().iter())
        .chain(p.keyword_rest())
        .chain(p.block().map(|b| b.as_node()))
        .collect();
    for n in nodes {
        out.push(match &n {
            Node::RequiredParameterNode { .. } => MethodParam::Req(name(n.as_required_parameter_node().unwrap().name())),
            Node::OptionalParameterNode { .. } => MethodParam::Opt(name(n.as_optional_parameter_node().unwrap().name())),
            Node::RestParameterNode { .. } => MethodParam::Rest(n.as_rest_parameter_node().unwrap().name().map(name)),
            Node::RequiredKeywordParameterNode { .. } => {
                MethodParam::Kw(name(n.as_required_keyword_parameter_node().unwrap().name()))
            }
            Node::OptionalKeywordParameterNode { .. } => {
                MethodParam::KwOpt(name(n.as_optional_keyword_parameter_node().unwrap().name()))
            }
            Node::KeywordRestParameterNode { .. } => MethodParam::KwRest(n.as_keyword_rest_parameter_node().unwrap().name().map(name)),
            Node::BlockParameterNode { .. } => MethodParam::Block(n.as_block_parameter_node().unwrap().name().map(name)),
            Node::NoKeywordsParameterNode { .. } => MethodParam::NoKw,
            Node::ForwardingParameterNode { .. } => MethodParam::Forwarding,
            _ => return Err(Error::Other),
        });
    }
    Ok(out)
}

/// Why a Spoom translation is not done: the port does not cover it (the correction is withheld),
/// or Spoom/RBI raise (a cop error).
#[derive(Debug, PartialEq)]
pub enum Tr {
    Unsupported,
    Raise,
}

impl From<Error> for Tr {
    fn from(e: Error) -> Tr {
        match e {
            Error::Type | Error::Other => Tr::Raise,
        }
    }
}

// ---- RBS types (a subset of the rbs gem's parser) and RBI's RBS::TypeTranslator ----

struct RbsParser<'a> {
    s: &'a [u8],
    i: usize,
    vars: Vec<String>,
}

/// An RBS function (parameters and return type): positional (type, name) lists and keywords.
#[derive(Default)]
struct Function {
    required: Vec<(Type, Option<String>)>,
    optional: Vec<(Type, Option<String>)>,
    rest: Option<(Type, Option<String>)>,
    trailing: Vec<(Type, Option<String>)>,
    req_kw: Vec<(String, Type)>,
    opt_kw: Vec<(String, Type)>,
    rest_kw: Option<(Type, Option<String>, bool)>,
    returns: Type,
}

/// Parse results: Err(true) unsupported by the port, Err(false) an RBS::ParsingError.
type R<T> = Result<T, bool>;

const KEYWORDS: &[&str] = &[
    "void", "untyped", "bool", "nil", "top", "bot", "self", "instance", "class", "boolish", "singleton", "true", "false",
];

impl<'a> RbsParser<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn peek(&mut self, t: &str) -> bool {
        self.ws();
        self.s[self.i..].starts_with(t.as_bytes())
    }
    fn eat(&mut self, t: &str) -> bool {
        if self.peek(t) {
            self.i += t.len();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: &str) -> R<()> {
        if self.eat(t) { Ok(()) } else { Err(false) }
    }
    fn ident(&mut self) -> Option<String> {
        self.ws();
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_') {
            self.i += 1;
        }
        if start == self.i || self.s[start].is_ascii_digit() {
            self.i = start;
            return None;
        }
        Some(String::from_utf8_lossy(&self.s[start..self.i]).into_owned())
    }
    fn at_eof(&mut self) -> bool {
        self.ws();
        self.i >= self.s.len()
    }

    fn union(&mut self) -> R<Type> {
        let mut ts = vec![self.inter()?];
        while self.eat("|") {
            ts.push(self.inter()?);
        }
        Ok(if ts.len() == 1 { ts.pop().unwrap() } else { Type::Any(ts).simplify() })
    }
    fn inter(&mut self) -> R<Type> {
        let mut ts = vec![self.optional()?];
        while self.peek("&") && !self.peek("&&") {
            self.i += 1;
            ts.push(self.optional()?);
        }
        Ok(if ts.len() == 1 { ts.pop().unwrap() } else { Type::All(ts).simplify() })
    }
    fn optional(&mut self) -> R<Type> {
        let mut t = self.primary()?;
        while self.s.get(self.i) == Some(&b'?') {
            self.i += 1;
            t = Type::Nilable(Box::new(t)).simplify();
        }
        Ok(t)
    }
    fn args(&mut self) -> R<Vec<Type>> {
        let mut v = Vec::new();
        if self.eat("]") {
            return Err(false);
        }
        loop {
            v.push(self.union()?);
            if self.eat(",") {
                continue;
            }
            self.expect("]")?;
            return Ok(v);
        }
    }
    fn primary(&mut self) -> R<Type> {
        self.ws();
        if self.eat("(") {
            let t = self.union()?;
            self.expect(")")?;
            return Ok(t);
        }
        if self.eat("[") {
            if self.eat("]") {
                return Ok(Type::Tuple(Vec::new()));
            }
            return Ok(Type::Tuple(self.args()?));
        }
        if self.eat("^") {
            let f = self.function(true)?;
            return Ok(proc_of(f));
        }
        if self.peek("{") || self.peek("\"") || self.peek("'") || self.peek(":") || self.peek("-") || self.peek("`") {
            return Err(true);
        }
        if self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
            return Err(true);
        }
        let absolute = self.eat("::");
        let Some(first) = self.ident() else { return Err(false) };
        let mut segs = vec![first];
        while self.s[self.i..].starts_with(b"::") {
            self.i += 2;
            segs.push(self.ident().ok_or(false)?);
        }
        let last = segs.last().unwrap().clone();
        if !absolute && segs.len() == 1 && KEYWORDS.contains(&last.as_str()) {
            return Ok(match last.as_str() {
                "void" => Type::Void,
                "untyped" => Type::Untyped,
                "bool" => Type::Boolean,
                "nil" => Type::Simple("NilClass".into()),
                "top" => Type::Anything,
                "bot" => Type::NoReturn,
                "self" => Type::SelfType,
                "instance" => Type::AttachedClass,
                "class" => Type::Simple("Class".into()),
                "singleton" => {
                    self.expect("(")?;
                    let abs = self.eat("::");
                    let mut n = self.ident().ok_or(false)?;
                    while self.s[self.i..].starts_with(b"::") {
                        self.i += 2;
                        n = format!("{n}::{}", self.ident().ok_or(false)?);
                    }
                    self.expect(")")?;
                    if self.peek("[") {
                        return Err(true);
                    }
                    Type::ClassOf(Box::new(Type::Simple(if abs { format!("::{n}") } else { n })), Vec::new())
                }
                // `boolish`, literal booleans
                _ => return Err(true),
            });
        }
        let name = format!("{}{}", if absolute { "::" } else { "" }, segs.join("::"));
        let first_char = last.as_bytes()[0];
        if first_char == b'_' {
            // An interface: untyped (its arguments are parsed and dropped).
            if self.s.get(self.i) == Some(&b'[') {
                self.i += 1;
                self.args()?;
            }
            return Ok(Type::Untyped);
        }
        if first_char.is_ascii_lowercase() {
            // A type alias: camelized.
            if segs.len() > 1 || absolute || self.peek("[") {
                return Err(true);
            }
            static WORD: std::sync::LazyLock<regex::Regex> =
                std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)(?:^|_)([a-z\d]*)").unwrap());
            let camel = WORD.replace_all(&last, |c: &regex::Captures| {
                let m = &c[1];
                let mut ch = m.chars();
                ch.next().map(|f| f.to_uppercase().chain(ch).collect::<String>()).unwrap_or_default()
            });
            return Ok(Type::Simple(camel.into_owned()));
        }
        if !absolute && segs.len() == 1 && self.vars.contains(&last) {
            return Ok(Type::TypeParameter(last));
        }
        if self.s.get(self.i) == Some(&b'[') {
            self.i += 1;
            let args = self.args()?;
            let generic = match name.trim_start_matches("::") {
                n @ ("Array" | "Class" | "Enumerable" | "Enumerator" | "Enumerator::Chain" | "Enumerator::Lazy" | "Hash"
                | "Module" | "Set" | "Range") => format!("::T::{n}"),
                _ => name,
            };
            return Ok(Type::Generic(generic, args));
        }
        Ok(Type::Simple(name))
    }

    /// A parameter list `( ... )` (or none) and `-> type`; `proc` for `^`, no block allowed.
    fn function(&mut self, proc: bool) -> R<Function> {
        let mut f = Function::default();
        if self.eat("(") {
            if self.eat("?") {
                return Err(true);
            }
            if !self.eat(")") {
                loop {
                    self.param(&mut f)?;
                    if self.eat(",") {
                        continue;
                    }
                    self.expect(")")?;
                    break;
                }
            }
        }
        if proc && (self.peek("{") || self.peek("?{") || self.peek("[")) {
            return Err(true);
        }
        self.expect("->")?;
        f.returns = self.optional_return()?;
        Ok(f)
    }

    fn optional_return(&mut self) -> R<Type> {
        self.union()
    }

    fn param_name(&mut self) -> Option<String> {
        let save = self.i;
        self.ws();
        match self.ident() {
            Some(n) if n.as_bytes()[0].is_ascii_lowercase() || n.as_bytes()[0] == b'_' => Some(n),
            _ => {
                self.i = save;
                None
            }
        }
    }

    fn param(&mut self, f: &mut Function) -> R<()> {
        self.ws();
        // keyword: `name: T` / `?name: T`
        let save = self.i;
        let opt = self.eat("?");
        if let Some(n) = self.ident()
            && self.s.get(self.i) == Some(&b':')
            && self.s.get(self.i + 1) != Some(&b':')
        {
            self.i += 1;
            let t = self.union()?;
            if opt { f.opt_kw.push((n, t)) } else { f.req_kw.push((n, t)) }
            return Ok(());
        }
        self.i = save;
        if self.eat("**") {
            let t = self.union()?;
            let nil = t == Type::Simple("NilClass".into());
            let n = self.param_name();
            f.rest_kw = Some((t, n, nil));
            return Ok(());
        }
        if self.eat("*") {
            let t = self.union()?;
            let n = self.param_name();
            f.rest = Some((t, n));
            return Ok(());
        }
        let opt = self.eat("?");
        let t = self.union()?;
        let n = self.param_name();
        if opt {
            f.optional.push((t, n));
        } else if f.rest.is_some() || !f.optional.is_empty() {
            f.trailing.push((t, n));
        } else {
            f.required.push((t, n));
        }
        Ok(())
    }
}

/// TypeTranslator#translate_function: a proc type.
fn proc_of(f: Function) -> Type {
    let mut params: Vec<(String, Type)> = Vec::new();
    let mut set = |n: String, t: Type| match params.iter_mut().find(|(k, _)| *k == n) {
        Some(slot) => slot.1 = t,
        None => params.push((n, t)),
    };
    let mut index = 0;
    for (t, n) in f.required.into_iter().chain(f.optional).chain(f.rest).chain(f.trailing) {
        set(n.unwrap_or(format!("arg{index}")), t);
        index += 1;
    }
    for (n, t) in f.req_kw.into_iter().chain(f.opt_kw) {
        set(n, t);
        index += 1;
    }
    if let Some((t, n, _)) = f.rest_kw {
        set(n.unwrap_or(format!("arg_{index}")), t);
    }
    Type::Proc { params, returns: Box::new(f.returns), bind: None }
}

/// A parsed `#:` method signature: type params, function, block (required?, function).
struct MethodType {
    type_params: Vec<String>,
    function: Function,
    block: Option<(bool, Function)>,
}

fn parse_method_type(s: &str) -> R<MethodType> {
    let mut p = RbsParser { s: s.as_bytes(), i: 0, vars: Vec::new() };
    let mut type_params = Vec::new();
    if p.eat("[") {
        loop {
            let n = p.ident().ok_or(false)?;
            if p.peek("<") || p.peek("=") {
                return Err(true);
            }
            type_params.push(n);
            if p.eat(",") {
                continue;
            }
            p.expect("]")?;
            break;
        }
    }
    p.vars = type_params.clone();
    let mut function = Function::default();
    if p.eat("(") {
        if p.eat("?") {
            return Err(true);
        }
        if !p.eat(")") {
            loop {
                p.param(&mut function)?;
                if p.eat(",") {
                    continue;
                }
                p.expect(")")?;
                break;
            }
        }
    }
    let mut block = None;
    let required = !p.peek("?{");
    if p.eat("?{") || p.eat("{") {
        if p.peek("[") {
            return Err(true);
        }
        let f = p.function(true)?;
        p.expect("}")?;
        block = Some((required, f));
    }
    p.expect("->")?;
    function.returns = p.union()?;
    if !p.at_eof() {
        return Err(false);
    }
    Ok(MethodType { type_params, function, block })
}

fn parse_type_only(s: &str) -> R<Type> {
    let mut p = RbsParser { s: s.as_bytes(), i: 0, vars: Vec::new() };
    let t = p.union()?;
    if !p.at_eof() {
        return Err(false);
    }
    Ok(t)
}

/// MethodTypeTranslator: the sig of a method type for the def's parameters. Err(()) on an RBI
/// error (the signature is skipped).
fn translate_method_type(mt: MethodType, method: &[MethodParam]) -> Result<Sig, ()> {
    let mut sig = Sig { type_params: mt.type_params, ..Default::default() };
    let f = mt.function;
    let anon = |m: &MethodParam| match m {
        MethodParam::Rest(_) => Some("*".to_string()),
        MethodParam::KwRest(_) => Some("**".to_string()),
        MethodParam::Block(_) => Some("&".to_string()),
        _ => None,
    };
    let positional = |t: Type, n: Option<String>, index: usize| -> Result<(String, Type), ()> {
        let name = match n {
            Some(n) => n,
            None => {
                let m = method.get(index).ok_or(())?;
                m.name().map(str::to_string).or_else(|| anon(m)).ok_or(())?
            }
        };
        Ok((name, t))
    };
    let mut index = 0;
    for (t, n) in f.required.into_iter().chain(f.optional).chain(f.rest).chain(f.trailing) {
        sig.params.push(positional(t, n, index)?);
        index += 1;
    }
    for (n, t) in f.req_kw.into_iter().chain(f.opt_kw) {
        sig.params.push((n, t));
        index += 1;
    }
    if let Some((t, n, nil)) = f.rest_kw
        && !nil
    {
        sig.params.push(positional(t, n, index)?);
    }
    sig.returns = f.returns;
    if let Some((required, bf)) = mt.block {
        let Some(bp) = method.iter().find(|m| matches!(m, MethodParam::Block(_))) else { return Err(()) };
        let name = bp.name().unwrap_or("&").to_string();
        let t = proc_of(bf);
        sig.params.push((name, if required { t } else { Type::Nilable(Box::new(t)).simplify() }));
    }
    Ok(sig)
}

/// Spoom's RBS comment to sig translation (HumanReadableTranslator) of the comment lines above a
/// def or accessor (`rbs_source`, one comment per line), as the cop's translated prefix. None:
/// something the port does not translate.
pub fn rbs_comments_to_sigs(rbs_source: &str, node: &Node) -> Option<String> {
    let lines: Vec<&str> = rbs_source.split('\n').collect();
    let method = match method_params(node) {
        Ok(m) => m,
        Err(_) => return None,
    };
    let accessor = node.as_call_node().map(|c| crate::ast::method_name(&c).to_string());
    // node_rbs_comments: annotations and signatures (a `#: ` line and the `#|` lines below it).
    let mut annotations: Vec<&str> = Vec::new();
    let mut signatures: Vec<(usize, usize, String)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.starts_with("# @") {
            annotations.push(crate::helpers::ruby_string::strip(&l[1..]));
        } else if l.starts_with("#: ") {
            let mut s = crate::helpers::ruby_string::strip(&l[2..]).to_string();
            let mut end = i;
            while end + 1 < lines.len() && lines[end + 1].starts_with("#|") {
                end += 1;
                s = format!("{s}\n{}", &lines[end][2..]);
            }
            if !s.starts_with("type ") {
                if s.contains('#') {
                    return None;
                }
                signatures.push((i, end, s));
            }
            i = end;
        } else if !l.starts_with('#') && !l.is_empty() {
            return None;
        }
        i += 1;
    }
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let mut drop: Vec<usize> = Vec::new();
    for (start, end, s) in &signatures {
        let sig = if let Some(m) = &accessor {
            let t = match parse_type_only(s) {
                Ok(t) => t,
                Err(true) => return None,
                Err(false) => continue,
            };
            let mut sig = Sig { returns: t.clone(), ..Default::default() };
            if m == "attr_writer" {
                let c = node.as_call_node().unwrap();
                let args = crate::ast::call_args(&c);
                let [a] = args.as_slice() else { return None };
                let name = crate::ast::node_text(a);
                sig.params.push((name.get(1..).unwrap_or("").to_string(), t));
            }
            sig
        } else {
            let mt = match parse_method_type(s) {
                Ok(mt) => mt,
                Err(true) => return None,
                Err(false) => continue,
            };
            match translate_method_type(mt, &method) {
                Ok(sig) => sig,
                Err(()) => continue,
            }
        };
        let mut sig = sig;
        for a in &annotations {
            match *a {
                "@abstract" => sig.is_abstract = true,
                "@final" => sig.is_final = true,
                "@override" => sig.is_override = true,
                "@override(allow_incompatible: true)" => {
                    sig.is_override = true;
                    sig.allow_incompatible = Some("true");
                }
                "@override(allow_incompatible: :visibility)" => {
                    sig.is_override = true;
                    sig.allow_incompatible = Some(":visibility");
                }
                "@overridable" => sig.is_overridable = true,
                "@without_runtime" => sig.without_runtime = true,
                _ => {}
            }
        }
        if let Some(d) = node.as_def_node()
            && matches!(d.name().as_slice(), b"method_added" | b"singleton_method_added")
        {
            sig.without_runtime = true;
        }
        out[*start] = sig_string(&sig);
        drop.extend(start + 1..=*end);
    }
    let kept: Vec<String> = out.into_iter().enumerate().filter(|(i, _)| !drop.contains(i)).map(|(_, l)| l).collect();
    Some(kept.join("\n"))
}

/// A sig as SigBuilder reads it: the sig, its (name, type source) parameters and return type source.
type BuiltSig = (Sig, Vec<(String, String)>, String);

/// SigBuilder: an RBI::Sig from a sig call (parameter and return types as written).
fn sig_of(call: &ruby_prism::CallNode) -> Result<BuiltSig, Tr> {
    let mut sig = Sig::default();
    let mut params: Vec<(String, String)> = Vec::new();
    let mut returns = "void".to_string();
    sig.without_runtime = call.receiver().is_some_and(|r| crate::ast::node_text(&r).contains("T::Sig::WithoutRuntime"));
    for a in crate::ast::call_args(call) {
        sig.is_final = crate::ast::node_text(&a) == ":final";
    }
    let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return Ok((sig, params, returns)) };
    let Some(body) = block.body().and_then(|b| b.as_statements_node()) else { return Ok((sig, params, returns)) };
    let stmts: Vec<Node> = body.body().iter().collect();
    for stmt in stmts {
        let mut cur = stmt.as_call_node();
        while let Some(c) = cur {
            let args = crate::ast::call_args(&c);
            match crate::ast::method_name(&c) {
                "abstract" => sig.is_abstract = true,
                "override" => {
                    sig.is_override = true;
                    if !args.is_empty() {
                        return Err(Tr::Unsupported);
                    }
                }
                "overridable" => sig.is_overridable = true,
                "void" => returns = "void".into(),
                "returns" => {
                    if let Some(a) = args.first() {
                        returns = crate::ast::node_text(a).to_string();
                    }
                }
                "params" => {
                    for a in &args {
                        let elements: Vec<Node> = match (a.as_keyword_hash_node(), a.as_hash_node()) {
                            (Some(h), _) => h.elements().iter().collect(),
                            (_, Some(h)) => h.elements().iter().collect(),
                            _ => return Err(Tr::Unsupported),
                        };
                        for e in elements {
                            let Some(pair) = e.as_assoc_node() else { return Err(Tr::Unsupported) };
                            let key = pair.key();
                            let name = match key.as_symbol_node() {
                                Some(s) => String::from_utf8_lossy(s.unescaped()).into_owned(),
                                None => crate::ast::node_text(&key).trim_end_matches(':').to_string(),
                            };
                            params.push((name, crate::ast::node_text(&pair.value()).to_string()));
                        }
                    }
                }
                "type_parameters" => {
                    for a in &args {
                        sig.type_params.push(crate::ast::node_text(a).trim_start_matches(':').to_string());
                    }
                }
                _ => return Err(Tr::Unsupported),
            }
            cur = c.receiver().and_then(|r| r.as_call_node());
            if c.receiver().is_some() && cur.is_none() {
                return Err(Tr::Unsupported);
            }
        }
        if stmt.as_call_node().is_none() {
            return Err(Tr::Unsupported);
        }
    }
    Ok((sig, params, returns))
}

/// Spoom's sigs to RBS comments translation (`positional_names: false`) of `source`, the sig nodes'
/// region with the runtime receivers dropped, for the def or accessor `node`: the translated
/// prefix. Ok(None): a sig is left untranslated (the cop then makes no correction).
pub fn sigs_to_rbs_comments(source: &str, node: &Node) -> Result<Option<String>, Tr> {
    let result = ruby_prism::parse(source.as_bytes());
    if result.errors().next().is_some() {
        return Err(Tr::Unsupported);
    }
    let program = result.node();
    let stmts: Vec<Node> = program.as_program_node().unwrap().statements().body().iter().collect();
    let accessor = node.as_call_node().map(|c| crate::ast::method_name(&c).to_string());
    let method = method_params(node).map_err(|_| Tr::Unsupported)?;
    let mut parts: Vec<(usize, usize, String)> = Vec::new();
    // (sig, parameters, return type, start offset, written over several lines)
    let mut sigs: Vec<(BuiltSig, usize, bool)> = Vec::new();
    for st in &stmts {
        let Some(c) = st.as_call_node() else { return Err(Tr::Unsupported) };
        if crate::ast::method_name(&c) != "sig" {
            return Err(Tr::Unsupported);
        }
        let ok_recv = c.receiver().is_none_or(|r| crate::ast::node_text(&r).contains("T::Sig::WithoutRuntime"));
        if !ok_recv {
            return Ok(None);
        }
        let (sig, params, returns) = sig_of(&c)?;
        let loc = st.location();
        let multiline = source[loc.start_offset()..loc.end_offset()].contains('\n');
        sigs.push(((sig, params, returns), loc.start_offset(), multiline));
        parts.push((loc.start_offset(), loc.end_offset(), String::new()));
    }
    if sigs.is_empty() {
        return Ok(None);
    }
    if accessor.is_some() && sigs.iter().any(|s| s.0.0.is_abstract) {
        return Ok(None);
    }
    let mut annotations = String::new();
    let any = |f: &dyn Fn(&Sig) -> bool| sigs.iter().any(|s| f(&s.0.0));
    if any(&|s| s.without_runtime) {
        annotations.push_str("# @without_runtime\n");
    }
    if any(&|s| s.is_final) {
        annotations.push_str("# @final\n");
    }
    if any(&|s| s.is_abstract) {
        annotations.push_str("# @abstract\n");
    }
    if any(&|s| s.is_override) {
        annotations.push_str("# @override\n");
    }
    if any(&|s| s.is_overridable) {
        annotations.push_str("# @overridable\n");
    }
    for (k, ((sig, params, returns), start, multiline)) in sigs.into_iter().enumerate() {
        let mut sig = sig;
        let parse = |s: &str| parse_string(s).map_err(|_| Tr::Raise);
        sig.params = params.iter().map(|(n, t)| Ok((n.clone(), parse(t)?))).collect::<Result<_, Tr>>()?;
        sig.returns = parse(&returns)?;
        let text = match &accessor {
            Some(m) if m == "attr_writer" && !sig.params.is_empty() && sig.returns == Type::Void => {
                sig.params[0].1.rbs_string()?
            }
            Some(_) => sig.returns.rbs_string()?,
            None => print_method_sig(&method, &sig, multiline)?,
        };
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        let indent = " ".repeat(source[line_start..start].chars().count());
        let mut out = String::new();
        for (i, l) in text.split_inclusive('\n').enumerate() {
            if i == 0 {
                out.push_str(&format!("#: {l}"));
            } else {
                out.push_str(&format!("{indent}#| {l}"));
            }
        }
        if k == 0 {
            out = format!("{annotations}{out}");
        }
        parts[k].2 = out;
    }
    let mut result = source.to_string();
    for (s, e, t) in parts.into_iter().rev() {
        result.replace_range(s..e, &t);
    }
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rbs(s: &str) -> String {
        rbs_of(s).unwrap()
    }

    #[test]
    fn rbs_strings() {
        assert_eq!(rbs("T.nilable(T::Array[String])"), "Array[String]?");
        assert_eq!(rbs("T.any(String, Symbol)"), "(String | Symbol)");
        assert_eq!(rbs("T.class_of(Foo)"), "singleton(Foo)");
        assert_eq!(rbs("T.proc.params(x: Integer).returns(String)"), "^(Integer x) -> String");
        assert_eq!(rbs("T::Hash[Symbol, T::Boolean]"), "Hash[Symbol, bool]");
        assert_eq!(rbs("T.nilable(T.proc.void)"), "(^-> void)?");
        assert_eq!(rbs("{a: Integer, 'b' => String}"), "{a: Integer, \"b\" => String}");
        assert_eq!(parse_string("T.something_weird(Integer)"), Err(Error::Type));
        assert_eq!(parse_string("type"), Err(Error::Type));
        assert_eq!(
            Type::Any(vec![Type::Simple("NilClass".into()), Type::Simple("String".into())]).simplify().to_rbi(),
            "::T.nilable(String)"
        );
    }
}
