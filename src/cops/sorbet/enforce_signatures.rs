use crate::cops::sorbet_redundant_t_let::def_params;
use crate::helpers::comment_association::{self, Associator};
use crate::helpers::sorbet_rbi_type::{self as rbi, Error, MethodParam, Tr, Type};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;
use std::collections::HashMap;

pub const NAME: &str = "Sorbet/EnforceSignatures";
pub static COP: EnforceSignatures = EnforceSignatures;
pub struct EnforceSignatures;

const MSG_USE_RBS: &str = "Use RBS signature comments rather than sig blocks.";
const MSG_RBS: &str = "Each method is required to have an RBS signature.";
const MSG_BOTH: &str = "Each method is required to have a signature.";
const MSG_USE_SIG: &str = "Use sig block signatures rather than RBS signature comments.";
const MSG_SIG: &str = "Each method is required to have a sig block signature.";

// A minimal port of rubocop-sorbet's SignatureHelp (batch S2 owns the shared `sorbet_signature`
// helper; RedundantTLet uses this one too until it lands).

/// `(const (const {nil? cbase} :T) :Sig)`
fn t_sig(node: &Node) -> bool {
    node.as_constant_path_node().is_some_and(|p| {
        p.name().is_some_and(|n| n.as_slice() == b"Sig") && p.parent().is_some_and(|q| helpers::parser_parent::is_top_const(&q, "T"))
    })
}

/// `(const (const (const {nil? cbase} :T) :Sig) :WithoutRuntime)`
fn t_sig_without_runtime(node: &Node) -> bool {
    node.as_constant_path_node().is_some_and(|p| {
        p.name().is_some_and(|n| n.as_slice() == b"WithoutRuntime") && p.parent().is_some_and(|q| t_sig(&q))
    })
}

/// SignatureHelp `signature?`: `sig { }` / `sig(:final) { }` with no block parameters, bare or on
/// `T::Sig` / `T::Sig::WithoutRuntime`. Some(true) for the `T::Sig` receiver (`sig_with_runtime?`).
pub fn signature_kind(node: &Node) -> Option<bool> {
    let c = node.as_call_node()?;
    let b = c.block()?.as_block_node()?;
    if is_csend(&c) || method_name(&c) != "sig" {
        return None;
    }
    let no_params = match b.parameters() {
        None => true,
        Some(p) => p.as_block_parameters_node().is_some_and(|bp| bp.parameters().is_none() && bp.locals().iter().next().is_none()),
    };
    if !no_params {
        return None;
    }
    let args = call_args(&c);
    match args.as_slice() {
        [] => {}
        [a] if a.as_symbol_node().is_some_and(|s| s.unescaped() == b"final" && s.opening_loc().is_some()) => {}
        _ => return None,
    }
    match c.receiver() {
        None => Some(false),
        Some(r) if t_sig(&r) => Some(true),
        Some(r) if t_sig_without_runtime(&r) => Some(false),
        _ => None,
    }
}

pub fn is_signature(node: &Node) -> bool {
    signature_kind(node).is_some()
}

/// `(send nil? {:attr_reader :attr_writer :attr_accessor} ...)`
fn accessor<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| {
        !is_csend(c) && c.receiver().is_none() && matches!(method_name(c), "attr_reader" | "attr_writer" | "attr_accessor")
    })
}

type Scope = Option<(usize, usize, usize)>;

#[derive(Default)]
struct State {
    sigs: HashMap<Scope, Vec<(usize, usize)>>,
    /// Comment associations (`ast_with_comments`), built on first use.
    assoc: Option<Vec<(usize, usize, Vec<usize>)>>,
}

fn range(n: &Node) -> (usize, usize) {
    (n.location().start_offset(), n.location().end_offset())
}

/// A parser `begin`: a StatementsNode of 2+ statements (not a plain `begin..end` body) or parens.
fn is_parser_begin(anc: &[Node], i: usize) -> bool {
    match &anc[i] {
        Node::ParenthesesNode { .. } => true,
        Node::StatementsNode { .. } => !(i > 0
            && anc[i - 1]
                .as_begin_node()
                .is_some_and(|b| b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none())),
        _ => false,
    }
}

/// EnforceSignatures#scope: the nearest parser ancestor that is a begin, block, class or module.
fn scope(anc: &[Node], from: usize) -> Scope {
    let mut i = helpers::parser_parent::parser_parent_index(anc, from)?;
    loop {
        let n = &anc[i];
        let is_block = n.as_block_node().is_some_and(|b| {
            !b.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
        }) || n.as_lambda_node().is_some();
        if is_block || is_parser_begin(anc, i) || matches!(n, Node::ClassNode { .. } | Node::ModuleNode { .. }) {
            let (s, e) = range(n);
            return Some((kind::of(n), s, e));
        }
        i = helpers::parser_parent::parser_parent_index(anc, i)?;
    }
}

/// Climb while the parser parent is a send: the index of the outermost such send (None: the node).
fn leftmost_send(anc: &[Node], from: usize) -> Option<usize> {
    let mut at = None;
    let mut i = from;
    while let Some(p) = helpers::parser_parent::parser_parent_index(anc, i)
        && anc[p].as_call_node().is_some_and(|c| !is_csend(&c))
    {
        at = Some(p);
        i = p;
    }
    at
}

fn send_or_node_range(n: &Node) -> (usize, usize) {
    match n.as_call_node() {
        Some(c) => send_range(&c),
        None => range(n),
    }
}

/// RBSParser.rbs_signatures_before: the RBS signature groups (`#:` and its `#|` lines) in the
/// contiguous comment block above the node with range `r` (its associated leading comments).
fn rbs_signatures(cx: &mut Cx, r: (usize, usize)) -> Vec<Vec<(usize, usize)>> {
    let src = cx.src;
    let st = cx.state::<State>();
    if st.assoc.is_none() {
        let mut a = Associator::new(src, comment_association::skipped_directives(src));
        if let Some(root) = src.root.as_ref() {
            ruby_prism::Visit::visit(&mut a, root);
        }
        st.assoc = Some(a.nodes);
    }
    let nodes = st.assoc.as_ref().unwrap();
    let Some(entry) = nodes.iter().find(|n| n.0 == r.0 && n.1 == r.1) else { return Vec::new() };
    let line = src.line_of(r.0);
    let mut cs: Vec<(usize, usize)> =
        entry.2.iter().map(|&i| (src.comments[i].start, src.comments[i].end)).filter(|c| src.line_of(c.0) < line).collect();
    cs.sort_by_key(|c| src.line_of(c.0));
    let mut block = Vec::new();
    let mut expected = line;
    for c in cs.iter().rev() {
        if src.line_of(c.0) + 1 != expected {
            break;
        }
        block.insert(0, *c);
        expected = src.line_of(c.0);
    }
    let mut groups: Vec<Vec<(usize, usize)>> = Vec::new();
    for c in block {
        let t = src.slice(c.0, c.1);
        if t.starts_with("#:") {
            groups.push(vec![c]);
        } else if t.starts_with("#|")
            && let Some(g) = groups.last_mut()
        {
            g.push(c);
        }
    }
    groups
}

/// What the suggestion needs from the def or accessor.
enum Target {
    Def { params: Vec<(&'static str, String, Option<String>)>, initialize: bool },
    Accessor { method: String, param: Option<String> },
}

impl Cop for EnforceSignatures {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let anc = cx.ancestors;
        if node.as_call_node().is_some() {
            // on_block (SignatureHelp) runs before on_send.
            if is_signature(node) {
                let sc = scope(anc, anc.len());
                cx.state::<State>().sigs.entry(sc).or_default().push(range(node));
            }
            if accessor(node).is_none() {
                return;
            }
        }
        let sc = scope(anc, anc.len());
        self.check_node(cx, node, sc);
        cx.state::<State>().sigs.remove(&sc);
    }
}

impl EnforceSignatures {
    fn style(&self, cx: &Cx) -> Result<&'static str, ()> {
        match cx.cfg.params.get("Style") {
            Some(Value::Str(s)) => ["sig", "rbs", "both"].into_iter().find(|v| v == s).ok_or(()),
            Some(Value::Null) | None => Ok(if cx.cfg.params.get("AllowRBS") == Some(&Value::Bool(true)) { "both" } else { "sig" }),
            Some(_) => Err(()),
        }
    }

    fn check_node(&self, cx: &mut Cx, node: &Node, sc: Scope) {
        let anc = cx.ancestors;
        let src = cx.src;
        let sig_nodes: Vec<(usize, usize)> = cx.state::<State>().sigs.get(&sc).cloned().unwrap_or_default();
        // rbs_signatures_before climbs the sends; ast_with_comments keys the parser node.
        let lookup = match leftmost_send(anc, anc.len()) {
            Some(i) => range(&anc[i]),
            None => range(node),
        };
        let mut rbs = rbs_signatures(cx, lookup);
        if rbs.is_empty()
            && let Some(&first) = sig_nodes.first()
        {
            rbs = rbs_signatures(cx, first);
        }
        let Ok(style) = self.style(cx) else {
            return crate::runner::cop_error_at(NAME, src, crate::runner::visiting());
        };
        let r = send_or_node_range(node);
        match style {
            "rbs" => {
                if let Some(&first) = sig_nodes.first() {
                    if rbs.is_empty() {
                        match self.sigs_to_rbs(cx, node, &sig_nodes) {
                            Ok(Some((s, e, t))) => cx.add_fix(first.0, first.1, MSG_USE_RBS, |c| c.replace(s, e, &t)),
                            Ok(None) => cx.add_fix(first.0, first.1, MSG_USE_RBS, |_| {}),
                            Err(Tr::Unsupported) => cx.add_withheld_fix(first.0, first.1, MSG_USE_RBS),
                            Err(Tr::Raise) => crate::runner::cop_error_at(NAME, src, crate::runner::visiting()),
                        }
                    } else {
                        let (s, e) = range_by_whole_lines(src.text, first.0, sig_nodes.last().unwrap().1, true);
                        cx.add_fix(first.0, first.1, MSG_USE_RBS, |c| c.remove(s, e));
                    }
                    return;
                }
                if rbs.is_empty() {
                    self.insert(cx, node, r, MSG_RBS, Some("rbs"));
                }
            }
            "both" => {
                if sig_nodes.is_empty() && rbs.is_empty() {
                    let style = match cx.cfg.params.get("AutocorrectStyle") {
                        None | Some(Value::Null) => Some("sig"),
                        Some(Value::Str(s)) if s == "sig" => Some("sig"),
                        Some(Value::Str(s)) if s == "rbs" => Some("rbs"),
                        _ => None,
                    };
                    self.insert(cx, node, r, MSG_BOTH, style);
                }
            }
            _ => {
                if let Some(first_group) = rbs.first() {
                    let first = first_group[0];
                    let comments: Vec<(usize, usize)> = rbs.iter().flatten().copied().collect();
                    let (cs, ce) = self.rbs_correction_range(src, &comments);
                    if sig_nodes.is_empty() {
                        match self.rbs_to_sigs(cx, node, (cs, ce)) {
                            Some((s, e, t)) => cx.add_fix(first.0, first.1, MSG_USE_SIG, |c| c.replace(s, e, &t)),
                            None => cx.add_withheld_fix(first.0, first.1, MSG_USE_SIG),
                        }
                    } else {
                        let (s, e) = range_by_whole_lines(src.text, cs, ce, true);
                        cx.add_fix(first.0, first.1, MSG_USE_SIG, |c| c.remove(s, e));
                    }
                } else if sig_nodes.is_empty() {
                    self.insert(cx, node, r, MSG_SIG, Some("sig"));
                }
            }
        }
    }

    /// rbs_correction_range: from the first RBS comment (or the `# @` annotation lines right above
    /// it) to the last.
    fn rbs_correction_range(&self, src: &crate::source::Source, comments: &[(usize, usize)]) -> (usize, usize) {
        let mut first = comments[0];
        let mut expected = src.line_of(first.0);
        let first_line = expected;
        for c in src.comments.iter().rev() {
            let l = src.line_of(c.start);
            if l >= first_line {
                continue;
            }
            if l + 1 != expected || !src.slice(c.start, c.end).starts_with("# @") {
                break;
            }
            first = (c.start, c.end);
            expected = l;
        }
        (first.0, comments.last().unwrap().1)
    }

    fn target(&self, node: &Node) -> Result<Target, ()> {
        if let Some(def) = node.as_def_node() {
            let params = def_params(def.parameters())
                .into_iter()
                .map(|(k, n, r)| (k, n.map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default(), (k == "mlhs").then_some(r)))
                .map(|(k, n, r)| (k, n, r.map(|_| String::new())))
                .collect();
            let params = mlhs_names(def.parameters(), params);
            let initialize = def.receiver().is_none() && def.name().as_slice() == b"initialize";
            return Ok(Target::Def { params, initialize });
        }
        let c = accessor(node).unwrap();
        let method = method_name(&c).to_string();
        let first = call_args(&c).into_iter().next().or_else(|| c.block().filter(|b| b.as_block_argument_node().is_some()));
        let param = match first {
            Some(f) if method != "attr_reader" => Some(literal_value(&f)?),
            _ => None,
        };
        Ok(Target::Accessor { method, param })
    }

    /// autocorrect_with_signature_type: insert a suggestion before the leftmost send around the node.
    fn insert(&self, cx: &mut Cx, node: &Node, r: (usize, usize), msg: &str, style: Option<&str>) {
        let anc = cx.ancestors;
        let src = cx.src;
        let t = match leftmost_send(anc, anc.len()) {
            Some(i) => send_range(&anc[i].as_call_node().unwrap()),
            None => r,
        };
        let target = match self.target(node) {
            Ok(t) => t,
            Err(()) => return crate::runner::cop_error_at(NAME, src, crate::runner::visiting()),
        };
        let indent = src.column(t.0);
        let param = cx.cfg.str("ParameterTypePlaceholder").unwrap_or("T.untyped").to_string();
        let ret = cx.cfg.str("ReturnTypePlaceholder").unwrap_or("T.untyped").to_string();
        let initialize = matches!(target, Target::Def { initialize: true, .. }) && !in_sclass(anc);
        let text = match style {
            Some("sig") => Ok(sig_suggestion(&target, initialize, &param, &ret)),
            Some(_) => rbs_suggestion(&target, initialize, &param, &ret),
            None => Err(Error::Other),
        };
        match text {
            Ok(text) => {
                let text = format!("{text}\n{}", " ".repeat(indent));
                cx.add_fix(r.0, r.1, msg, |c| c.insert_before(t.0, t.1, &text));
            }
            Err(_) => crate::runner::cop_error_at(NAME, src, crate::runner::visiting()),
        }
    }

    /// autocorrect_rbs_to_sigs (Spoom's RBS comments to sigs translation); None: a signature the
    /// port does not translate (the correction is withheld).
    fn rbs_to_sigs(&self, cx: &Cx, node: &Node, (s, e): (usize, usize)) -> Option<(usize, usize, String)> {
        let src = cx.src;
        let rbs_source: String = src.slice(s, e).split_inclusive('\n').map(helpers::ruby_string::lstrip).collect();
        let translated = rbi::rbs_comments_to_sigs(&rbs_source, node)?;
        let replacement = helpers::ruby_string::rstrip(&translated).to_string();
        let indent = " ".repeat(src.column(s));
        Some((s, e, replacement.replace('\n', &format!("\n{indent}"))))
    }

    /// autocorrect_sigs_to_rbs: Ok(None) when the translation leaves a sig (no correction).
    fn sigs_to_rbs(&self, cx: &Cx, node: &Node, sigs: &[(usize, usize)]) -> Result<Option<(usize, usize, String)>, Tr> {
        let src = cx.src;
        let (s, e) = (sigs[0].0, sigs.last().unwrap().1);
        // normalize_runtime_sig_receivers: drop `T::Sig.` before a `sig` with runtime.
        let mut source = src.slice(s, e).to_string();
        for &(ss, _) in sigs.iter().rev() {
            let Some(n) = src.node_at(*sigs.iter().find(|r| r.0 == ss).unwrap()) else { return Err(Tr::Unsupported) };
            if signature_kind(n) == Some(true) {
                let c = n.as_call_node().unwrap();
                let recv = c.receiver().unwrap().location().start_offset();
                let sel = c.message_loc().unwrap().start_offset();
                source.replace_range(recv - s..sel - s, "");
            }
        }
        let out = rbi::sigs_to_rbs_comments(&source, node)?;
        Ok(out.map(|t| (s, e, helpers::ruby_string::rstrip(&t).to_string())))
    }
}

/// `in_sclass_context?`: a singleton class before any class or module around the def.
fn in_sclass(anc: &[Node]) -> bool {
    for a in anc.iter().rev() {
        match a {
            Node::SingletonClassNode { .. } => return true,
            Node::ClassNode { .. } | Node::ModuleNode { .. } => return false,
            _ => {}
        }
    }
    false
}

/// `symbol.value` for an accessor's first argument; Err where `value` is undefined (NoMethodError).
fn literal_value(n: &Node) -> Result<String, ()> {
    match n {
        Node::SymbolNode { .. } => Ok(String::from_utf8_lossy(n.as_symbol_node().unwrap().unescaped()).into_owned()),
        Node::StringNode { .. } => Ok(String::from_utf8_lossy(n.as_string_node().unwrap().unescaped()).into_owned()),
        Node::IntegerNode { .. } | Node::FloatNode { .. } => Ok(node_text(n).replace('_', "")),
        _ => Err(()),
    }
}

/// The parser names of destructured parameters: `arg.children.first.to_s`, an s-expression.
fn mlhs_names(
    params: Option<ruby_prism::ParametersNode>,
    mut out: Vec<(&'static str, String, Option<String>)>,
) -> Vec<(&'static str, String, Option<String>)> {
    fn sexp(n: &Node, indent: usize) -> String {
        let pad = "  ".repeat(indent);
        match n {
            Node::RequiredParameterNode { .. } => {
                format!("{pad}(arg :{})", String::from_utf8_lossy(n.as_required_parameter_node().unwrap().name().as_slice()))
            }
            Node::SplatNode { .. } => match n.as_splat_node().unwrap().expression() {
                Some(e) => format!("{pad}(restarg :{})", node_text(&e)),
                None => format!("{pad}(restarg)"),
            },
            Node::MultiTargetNode { .. } => {
                let m = n.as_multi_target_node().unwrap();
                let mut s = format!("{pad}(mlhs");
                for c in m.lefts().iter().chain(m.rest()).chain(m.rights().iter()) {
                    if c.as_implicit_rest_node().is_some() {
                        continue;
                    }
                    s.push_str(&format!("\n{}", sexp(&c, indent + 1)));
                }
                s + ")"
            }
            _ => format!("{pad}{}", node_text(n)),
        }
    }
    let Some(p) = params else { return out };
    let mlhs: Vec<Node> = p.requireds().iter().chain(p.posts().iter()).filter(|n| n.as_multi_target_node().is_some()).collect();
    let mut it = mlhs.iter();
    for o in out.iter_mut().filter(|o| o.0 == "mlhs") {
        if let Some(m) = it.next() {
            let m = m.as_multi_target_node().unwrap();
            let first = m.lefts().iter().next().or_else(|| m.rest()).or_else(|| m.rights().iter().next());
            o.1 = first.map(|f| sexp(&f, 0)).unwrap_or_default();
        }
    }
    out
}

/// SigSuggestion#to_autocorrect (without the trailing newline and indentation).
fn sig_suggestion(t: &Target, initialize: bool, param: &str, ret: &str) -> String {
    let (names, returns): (Vec<String>, Option<&str>) = match t {
        Target::Def { params, .. } => (params.iter().map(|p| p.1.clone()).collect(), initialize.then_some("void")),
        Target::Accessor { method, param: p } => {
            let names = match p {
                Some(n) if method != "attr_reader" => vec![n.clone()],
                _ => Vec::new(),
            };
            (names, (method == "attr_writer").then_some("void"))
        }
    };
    let params = if names.is_empty() {
        String::new()
    } else {
        format!("params({}).", names.iter().map(|n| format!("{n}: {param}")).collect::<Vec<_>>().join(", "))
    };
    let returns = match returns {
        Some(_) => "void".to_string(),
        None => format!("returns({ret})"),
    };
    format!("sig {{ {params}{returns} }}")
}

/// RBSSuggestion#to_autocorrect (without the trailing newline and indentation).
fn rbs_suggestion(t: &Target, initialize: bool, param: &str, ret: &str) -> Result<String, Error> {
    let param_type = rbi::parse_string(param)?;
    let return_type = rbi::parse_string(ret)?;
    match t {
        Target::Accessor { method, .. } => {
            let ty = if method == "attr_writer" { &param_type } else { &return_type };
            Ok(format!("#: {}", ty.rbs_string()?))
        }
        Target::Def { params, .. } => {
            let mut method: Vec<MethodParam> = Vec::new();
            let mut sig: Vec<(String, Type)> = Vec::new();
            for (kind, name, _) in params {
                let anon = name.is_empty();
                let named = |n: &str| if anon { n.to_string() } else { name.clone() };
                let mut add = |m: MethodParam, s: Option<String>| {
                    method.push(m);
                    if let Some(s) = s {
                        sig.push((s, param_type.clone()));
                    }
                };
                match *kind {
                    "arg" => add(MethodParam::Req(name.clone()), Some(name.clone())),
                    "optarg" => add(MethodParam::Opt(name.clone()), Some(name.clone())),
                    "restarg" => add(MethodParam::Rest((!anon).then(|| name.clone())), Some(named("*"))),
                    "kwarg" => add(MethodParam::Kw(name.clone()), Some(name.clone())),
                    "kwoptarg" => add(MethodParam::KwOpt(name.clone()), Some(name.clone())),
                    "kwrestarg" => add(MethodParam::KwRest((!anon).then(|| name.clone())), Some(named("**"))),
                    "blockarg" => add(MethodParam::Block((!anon).then(|| name.clone())), Some(named("&"))),
                    "forward_arg" => {
                        add(MethodParam::Rest(None), Some("*".into()));
                        add(MethodParam::KwRest(None), Some("**".into()));
                        add(MethodParam::Block(None), Some("&".into()));
                    }
                    "kwnilarg" => add(MethodParam::NoKw, None),
                    _ => add(MethodParam::Req("untyped".into()), Some("untyped".into())),
                }
            }
            let returns = if initialize { Type::Void } else { return_type };
            let s = rbi::Sig { params: sig, returns, ..Default::default() };
            Ok(format!("#: {}", rbi::print_method_sig(&method, &s, false)?))
        }
    }
}
