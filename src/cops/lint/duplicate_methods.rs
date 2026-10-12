use crate::helpers::const_name::const_name;
use crate::prelude::*;
use std::collections::{HashMap, HashSet};

pub const NAME: &str = "Lint/DuplicateMethods";
pub static COP: DuplicateMethods = DuplicateMethods;
pub struct DuplicateMethods;

// ponytail: `UseProjectIndex` (cross-file duplicates) and `def Const.name` whose constant is not an
// enclosing class (RuboCop keys those by the defs node's s-expression) are not ported.

/// Per-file state: `@definitions` (key -> (method name line, offense start)) and `@scopes`.
#[derive(Default)]
struct State {
    definitions: HashMap<String, usize>,
    rescue: HashSet<String>,
    ensure: HashSet<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PT {
    Class,
    Module,
    Sclass,
    Casgn,
    /// `block` (lambda included); `numbered` for numblock / itblock.
    Block { numbered: bool },
    Def,
    Defs,
    Rescue,
    Ensure,
    If,
    Lvasgn,
    Ivasgn,
    Begin,
    Call,
    Other,
}

/// A parser-gem ancestor: its type and the Prism ancestor index it comes from (for a block: the
/// BlockNode / LambdaNode, or the owning CallNode when reached through its receiver or arguments).
#[derive(Clone, Copy, Debug)]
struct PA {
    t: PT,
    i: usize,
}

/// PathUtil.smart_path of the buffer name: the display path is relative to the working directory, and
/// a file outside it (`../x.rb`) prints as RuboCop's expanded absolute path instead.
fn smart_path(display: &str) -> String {
    if !display.starts_with("../") {
        return display.to_string();
    }
    let mut abs = std::env::current_dir().unwrap_or_default();
    for part in std::path::Path::new(display).components() {
        match part {
            std::path::Component::ParentDir => {
                abs.pop();
            }
            other => abs.push(other),
        }
    }
    abs.display().to_string()
}

fn numbered(block: &ruby_prism::BlockNode) -> bool {
    block.parameters().is_some_and(|p| p.as_numbered_parameters_node().is_some() || p.as_it_parameters_node().is_some())
}

fn literal_block<'pr>(node: &Node<'pr>) -> Option<ruby_prism::BlockNode<'pr>> {
    let b = match kind::of(node) {
        kind::CALL_NODE => node.as_call_node().unwrap().block(),
        kind::SUPER_NODE => node.as_super_node().unwrap().block(),
        kind::FORWARDING_SUPER_NODE => node.as_forwarding_super_node().unwrap().block().map(|b| b.as_node()),
        _ => None,
    };
    b.and_then(|b| b.as_block_node())
}

fn same(a: &Node, b: &Node) -> bool {
    let (x, y) = (a.location(), b.location());
    (x.start_offset(), x.end_offset(), kind::of(a)) == (y.start_offset(), y.end_offset(), kind::of(b))
}

/// Parser-shaped ancestors (outermost first) of the node whose Prism ancestors are `anc`.
fn parser_ancestors(anc: &[Node], subject: &Node) -> Vec<PA> {
    let mut out = Vec::new();
    for (i, n) in anc.iter().enumerate() {
        let next = anc.get(i + 1).unwrap_or(subject);
        let t = match kind::of(n) {
            kind::CALL_NODE | kind::SUPER_NODE | kind::FORWARDING_SUPER_NODE => match literal_block(n) {
                Some(b) if same(&b.as_node(), next) => continue,
                Some(b) => {
                    out.push(PA { t: PT::Block { numbered: numbered(&b) }, i });
                    if kind::of(n) == kind::CALL_NODE { PT::Call } else { PT::Other }
                }
                None if kind::of(n) == kind::CALL_NODE => PT::Call,
                None => PT::Other,
            },
            kind::BLOCK_NODE => PT::Block { numbered: numbered(&n.as_block_node().unwrap()) },
            kind::LAMBDA_NODE => PT::Block { numbered: false },
            kind::CLASS_NODE => PT::Class,
            kind::MODULE_NODE => PT::Module,
            kind::SINGLETON_CLASS_NODE => PT::Sclass,
            kind::CONSTANT_WRITE_NODE | kind::CONSTANT_PATH_WRITE_NODE => PT::Casgn,
            kind::DEF_NODE => {
                if n.as_def_node().unwrap().receiver().is_some() {
                    PT::Defs
                } else {
                    PT::Def
                }
            }
            kind::IF_NODE | kind::UNLESS_NODE => PT::If,
            kind::LOCAL_VARIABLE_WRITE_NODE => PT::Lvasgn,
            kind::INSTANCE_VARIABLE_WRITE_NODE => PT::Ivasgn,
            kind::PARENTHESES_NODE | kind::EMBEDDED_STATEMENTS_NODE => PT::Begin,
            kind::RESCUE_MODIFIER_NODE => PT::Rescue,
            kind::BEGIN_NODE => {
                let b = n.as_begin_node().unwrap();
                if b.begin_keyword_loc().is_some() {
                    out.push(PA { t: PT::Other, i });
                }
                if b.ensure_clause().is_some() {
                    out.push(PA { t: PT::Ensure, i });
                }
                if b.rescue_clause().is_some() && next.as_ensure_node().is_none() {
                    out.push(PA { t: PT::Rescue, i });
                }
                continue;
            }
            kind::STATEMENTS_NODE => {
                let flat = i > 0
                    && anc[i - 1].as_begin_node().is_some_and(|b| {
                        b.begin_keyword_loc().is_some() && b.rescue_clause().is_none() && b.ensure_clause().is_none()
                    });
                if flat || n.as_statements_node().unwrap().body().iter().count() < 2 {
                    continue;
                }
                PT::Begin
            }
            // Later `rescue` clauses are siblings of the first in parser.
            kind::RESCUE_NODE if next.as_rescue_node().is_some() => continue,
            kind::PROGRAM_NODE | kind::ARGUMENTS_NODE | kind::ELSE_NODE | kind::ENSURE_NODE => continue,
            _ => PT::Other,
        };
        out.push(PA { t, i });
    }
    out
}

/// The `send` of a block ancestor: (method name, receiver).
fn block_send<'pr>(anc: &[Node<'pr>], pa: PA) -> (String, Option<Node<'pr>>) {
    let n = &anc[pa.i];
    let owner = match kind::of(n) {
        kind::LAMBDA_NODE => return ("lambda".into(), None),
        kind::BLOCK_NODE => &anc[pa.i - 1],
        _ => n,
    };
    match owner.as_call_node() {
        Some(c) => (String::from_utf8_lossy(c.name().as_slice()).into_owned(), c.receiver()),
        None => ("super".into(), None),
    }
}

/// The block's send arguments count (parser args, block pass included).
fn block_send_nargs(anc: &[Node], pa: PA) -> usize {
    let n = &anc[pa.i];
    let owner = if kind::of(n) == kind::BLOCK_NODE { &anc[pa.i - 1] } else { n };
    owner.as_call_node().map_or(0, |c| call_args(&c).len())
}

fn is_const(n: &Node) -> bool {
    n.as_constant_read_node().is_some() || n.as_constant_path_node().is_some()
}

/// `#global_const?(name)`: `(const {nil? cbase} name)`.
fn global_const(n: &Node, names: &[&str]) -> bool {
    (n.as_constant_read_node().is_some() || n.as_constant_path_node().is_some_and(|p| p.parent().is_none()))
        && const_name(n).is_some_and(|c| names.contains(&c.as_str()))
}

/// `(send #global_const?(names) :new ...)` / its block, for a casgn value.
fn class_new_value(value: &Node, names: &[&str]) -> bool {
    value.as_call_node().is_some_and(|c| {
        !is_csend(&c) && c.name().as_slice() == b"new" && c.receiver().is_some_and(|r| global_const(&r, names))
    })
}

/// `(block (send (const _ {names}) :new ...) ...)` for a block ancestor.
fn class_or_module_new_block(anc: &[Node], pa: PA, names: &[&str]) -> bool {
    if pa.t != (PT::Block { numbered: false }) || kind::of(&anc[pa.i]) == kind::LAMBDA_NODE {
        return false;
    }
    let (name, recv) = block_send(anc, pa);
    let owner_is_send = {
        let n = &anc[pa.i];
        let o = if kind::of(n) == kind::BLOCK_NODE { &anc[pa.i - 1] } else { n };
        o.as_call_node().is_some_and(|c| !is_csend(&c))
    };
    owner_is_send
        && name == "new"
        && recv.is_some_and(|r| short_const(&r).is_some_and(|s| names.contains(&s.as_str())))
}

/// The last segment of a constant (`(const _ name)`).
fn short_const(r: &Node) -> Option<String> {
    match r.as_constant_path_node() {
        Some(p) => p.name().map(|n| String::from_utf8_lossy(n.as_slice()).into_owned()),
        None => r.as_constant_read_node().map(|c| String::from_utf8_lossy(c.name().as_slice()).into_owned()),
    }
}

/// Namespace of a `defined_module`: nil, cbase, or a node.
enum Ns<'pr> {
    Nil,
    Cbase,
    N(Node<'pr>),
}

impl Ns<'_> {
    /// `namespace.const_name` interpolated (nil -> "").
    fn const_name(&self) -> String {
        match self {
            Ns::N(n) => const_name(n).unwrap_or_default(),
            _ => String::new(),
        }
    }
}

fn split_const<'pr>(n: Node<'pr>) -> Option<(Ns<'pr>, String)> {
    if let Some(c) = n.as_constant_read_node() {
        return Some((Ns::Nil, String::from_utf8_lossy(c.name().as_slice()).into_owned()));
    }
    let p = n.as_constant_path_node()?;
    let name = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    Some((p.parent().map_or(Ns::Cbase, Ns::N), name))
}

/// `defined_module` children (namespace, name) of a class / module / casgn ancestor.
fn defined_module<'pr>(n: &Node<'pr>) -> Option<(Ns<'pr>, String)> {
    match kind::of(n) {
        kind::CLASS_NODE => split_const(n.as_class_node().unwrap().constant_path()),
        kind::MODULE_NODE => split_const(n.as_module_node().unwrap().constant_path()),
        kind::CONSTANT_WRITE_NODE | kind::CONSTANT_PATH_WRITE_NODE => {
            let (target, value) = match n.as_constant_write_node() {
                Some(w) => (None, w.value()),
                None => {
                    let w = n.as_constant_path_write_node().unwrap();
                    (Some(w.target()), w.value())
                }
            };
            let names = ["Class", "Module"];
            let ok = class_new_value(&value, &names)
                || value.as_call_node().is_some_and(|c| {
                    c.block().is_some_and(|b| b.as_block_node().is_some_and(|b| !numbered(&b)))
                        && !is_csend(&c)
                        && c.name().as_slice() == b"new"
                        && c.receiver().is_some_and(|r| global_const(&r, &names))
                });
            if !ok {
                return None;
            }
            match target {
                None => {
                    let w = n.as_constant_write_node().unwrap();
                    Some((Ns::Nil, String::from_utf8_lossy(w.name().as_slice()).into_owned()))
                }
                Some(t) => split_const(t.as_node()),
            }
        }
        _ => None,
    }
}

fn defined_module_name(n: &Node) -> Option<String> {
    let (ns, name) = defined_module(n)?;
    Some(match ns {
        Ns::Nil | Ns::Cbase => name,
        ns => format!("{}::{name}", ns.const_name()),
    })
}

/// `parent_module_name` of the node whose parser ancestors are `pas`.
fn parent_module_name(anc: &[Node], pas: &[PA]) -> Option<String> {
    let mut parts = Vec::new();
    for j in (0..pas.len()).rev() {
        let pa = pas[j];
        let n = &anc[pa.i];
        match pa.t {
            PT::Class | PT::Module | PT::Casgn => parts.extend(defined_module_name(n)),
            PT::Sclass => {
                let subject = n.as_singleton_class_node().unwrap().expression();
                if is_const(&subject) {
                    parts.push(format!("#<Class:{}>", const_name(&subject).unwrap_or_default()));
                } else if subject.as_self_node().is_some() {
                    parts.push(format!("#<Class:{}>", parent_module_name(anc, &pas[..j]).unwrap_or_default()));
                } else {
                    return None;
                }
            }
            PT::Block { numbered: false } => {
                let (name, recv) = block_send(anc, pa);
                if name == "class_eval" {
                    match recv {
                        None => {}
                        Some(r) if is_const(&r) => parts.push(const_name(&r).unwrap_or_default()),
                        Some(_) => return None,
                    }
                } else {
                    // new_class_or_module_block?: ^(casgn _ _ (block (send (const _ {:Class :Module}) :new) ...))
                    let casgn_parent = j > 0 && pas[j - 1].t == PT::Casgn;
                    if !(casgn_parent
                        && block_send_nargs(anc, pa) == 0
                        && class_or_module_new_block(anc, pa, &["Class", "Module"]))
                    {
                        return None;
                    }
                }
            }
            _ => {}
        }
    }
    parts.reverse();
    let r = parts.join("::");
    Some(if r.is_empty() { "Object".into() } else { r })
}

/// `humanize_scope`: `X::#<Class:X>` / `#<Class:X>` become `X.`; otherwise append `#`.
fn humanize_scope(scope: &str) -> String {
    let s = scope.as_bytes();
    let tag = "#<Class:";
    for p in 0..=s.len() {
        if !scope.is_char_boundary(p) {
            continue;
        }
        // (?<name>.*)::#<Class:\k<name}>  (greedy name, no newlines)
        let line_end = scope[p..].find('\n').map_or(scope.len(), |e| p + e);
        for q in (p..=line_end).rev() {
            if !scope.is_char_boundary(q) {
                continue;
            }
            let name = &scope[p..q];
            let want = format!("::{tag}{name}>");
            if scope[q..].starts_with(&want) {
                let r = format!("{}{name}.{}", &scope[..p], &scope[q + want.len()..]);
                return finish(r);
            }
        }
        // #<Class:(?<name>.*)>(?:::)?
        if scope[p..].starts_with(tag) {
            let body = &scope[p + tag.len()..line_end];
            if let Some(gt) = body.rfind('>') {
                let name = &body[..gt];
                let mut end = p + tag.len() + gt + 1;
                if scope[end..].starts_with("::") {
                    end += 2;
                }
                let r = format!("{}{name}.{}", &scope[..p], &scope[end..]);
                return finish(r);
            }
        }
    }
    finish(scope.to_string())
}

fn finish(s: String) -> String {
    if s.ends_with('.') { s } else { format!("{s}#") }
}

/// `qualified_name(enclosing, namespace, mod_name)`
fn qualified_name(enclosing: Option<&str>, ns: Option<String>, mod_name: &str) -> String {
    if enclosing != Some("Object") {
        let e = enclosing.unwrap_or("");
        match ns {
            Some(ns) => format!("{e}::{ns}::{mod_name}"),
            None => format!("{e}::{mod_name}"),
        }
    } else if let Some(ns) = ns {
        format!("{ns}::{mod_name}")
    } else {
        mod_name.to_string()
    }
}

struct Ctx<'a, 'pr> {
    anc: &'a [Node<'pr>],
    pas: Vec<PA>,
}

impl<'pr> Ctx<'_, 'pr> {
    fn inside_if(&self) -> bool {
        self.pas.iter().any(|p| p.t == PT::If)
    }

    /// `anonymous_class_block`: index into `pas`.
    fn anonymous_class_block(&self) -> Option<usize> {
        let j = self.pas.iter().rposition(|p| p.t == PT::Block { numbered: false })?;
        if !class_or_module_new_block(self.anc, self.pas[j], &["Class", "Module"]) {
            return None;
        }
        if j > 0 && self.pas[j - 1].t == PT::Lvasgn {
            return None;
        }
        let bad_sclass = self.pas.iter().any(|p| {
            p.t == PT::Sclass && self.anc[p.i].as_singleton_class_node().unwrap().expression().as_self_node().is_none()
        });
        (!bad_sclass).then_some(j)
    }

    /// `anon_block_scope_id`
    fn anon_block_scope_id(&self, j: usize, src: &crate::source::Source) -> Option<String> {
        let identity = || Some(format!("block@{}", self.anc[self.pas[j].i].location().start_offset()));
        if j == 0 {
            return None;
        }
        let parent = self.pas[j - 1];
        let (receiver, method) = match parent.t {
            PT::Block { .. } => {
                let (name, recv) = block_send(self.anc, parent);
                (recv, name)
            }
            PT::Call => {
                let c = self.anc[parent.i].as_call_node().unwrap();
                (c.receiver(), String::from_utf8_lossy(c.name().as_slice()).into_owned())
            }
            PT::Def | PT::Defs => {
                let d = self.anc[parent.i].as_def_node().unwrap();
                (d.receiver(), String::from_utf8_lossy(d.name().as_slice()).into_owned())
            }
            PT::Begin | PT::Casgn | PT::Ivasgn => (None, String::new()),
            _ => return None,
        };
        // scope_receiver: nil for a Class.new block inside a call; else named_receiver.
        let class_new = class_or_module_new_block(self.anc, self.pas[j], &["Class"]);
        let receiver = if class_new && parent.t == PT::Call { None } else { receiver };
        let receiver = receiver.filter(|r| {
            // `receiver unless class_or_module_new_block?(receiver)`
            !literal_block(r).is_some_and(|b| {
                !numbered(&b)
                    && r.as_call_node().is_some_and(|c| {
                        !is_csend(&c)
                            && c.name().as_slice() == b"new"
                            && c.receiver().is_some_and(|x| short_const(&x).is_some_and(|s| s == "Class" || s == "Module"))
                    })
            })
        });
        if let Some(r) = receiver {
            let l = r.location();
            return Some(format!("{}.{method}", src.slice(l.start_offset(), l.end_offset())));
        }
        let grand_block = j >= 2 && matches!(self.pas[j - 2].t, PT::Block { .. });
        if parent.t != PT::Begin || grand_block { identity() } else { None }
    }

    fn method_key(&self, name: &str) -> String {
        match self.pas.iter().rev().find(|p| matches!(p.t, PT::Def | PT::Defs)) {
            Some(p) => format!("{}.{name}", String::from_utf8_lossy(self.anc[p.i].as_def_node().unwrap().name().as_slice())),
            None => name.to_string(),
        }
    }

    fn handler(&self) -> Option<PT> {
        self.pas.iter().rev().find(|p| matches!(p.t, PT::Rescue | PT::Ensure)).map(|p| p.t)
    }
}

struct Found {
    method: String,
    key: String,
    handler: Option<PT>,
}

impl<'pr> Ctx<'_, 'pr> {
    fn found_method(&self, method: String, scope_id: Option<String>) -> Found {
        let mut key = self.method_key(&method);
        if let Some(id) = scope_id {
            key = format!("{key}@{id}");
        }
        Found { method, key, handler: self.handler() }
    }

    fn found_instance_method(&self, name: &str, src: &crate::source::Source) -> Option<Found> {
        if let Some(scope) = parent_module_name(self.anc, &self.pas) {
            return Some(self.found_method(format!("{}{name}", humanize_scope(&scope)), None));
        }
        if let Some(j) = self.anonymous_class_block() {
            let enclosing = parent_module_name(self.anc, &self.pas[..j]);
            let base = qualified_name(enclosing.as_deref(), None, "Object");
            let scope = if self.pas.iter().any(|p| p.t == PT::Sclass) { format!("#<Class:{base}>") } else { base };
            let id = self.anon_block_scope_id(j, src);
            return Some(self.found_method(format!("{}{name}", humanize_scope(&scope)), id));
        }
        // found_sclass_method
        let s = self.pas.iter().rev().find(|p| p.t == PT::Sclass)?;
        let subject = self.anc[s.i].as_singleton_class_node().unwrap().expression();
        let call = subject.as_call_node().filter(|c| !is_csend(c))?;
        Some(self.found_method(format!("{}.{name}", String::from_utf8_lossy(call.name().as_slice())), None))
    }

    fn found_self_method(&self, name: &str, src: &crate::source::Source) -> Option<Found> {
        if let Some(enclosing) = parent_module_name(self.anc, &self.pas) {
            return Some(self.found_method(format!("{enclosing}.{name}"), None));
        }
        let j = self.anonymous_class_block()?;
        let scope = qualified_name(parent_module_name(self.anc, &self.pas[..j]).as_deref(), None, "Object");
        let id = self.anon_block_scope_id(j, src);
        Some(self.found_method(format!("{scope}.{name}"), id))
    }

    /// `lookup_constant`
    fn found_const_method(&self, const_short: &str, name: &str) -> Option<Found> {
        for j in (0..self.pas.len()).rev() {
            let pa = self.pas[j];
            if !matches!(pa.t, PT::Class | PT::Module | PT::Casgn) {
                continue;
            }
            let Some((mut ns, mut mod_name)) = defined_module(&self.anc[pa.i]) else { continue };
            loop {
                if mod_name == const_short {
                    let ns_name = match &ns {
                        Ns::Nil => None,
                        other => Some(other.const_name()),
                    };
                    let enclosing = parent_module_name(self.anc, &self.pas[..j]);
                    let q = qualified_name(enclosing.as_deref(), ns_name, &mod_name);
                    return Some(self.found_method(format!("{q}.{name}"), None));
                }
                let Ns::N(n) = ns else { break };
                match split_const(n) {
                    Some((a, b)) => (ns, mod_name) = (a, b),
                    None => break,
                }
            }
        }
        None
    }
}

fn sym_value(n: &Node) -> Option<String> {
    n.as_symbol_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
}

fn str_or_sym_value(n: &Node) -> Option<String> {
    sym_value(n).or_else(|| n.as_string_node().map(|s| String::from_utf8_lossy(s.unescaped()).into_owned()))
}

impl Cop for DuplicateMethods {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::ALIAS_METHOD_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let ctx = Ctx { anc: cx.ancestors, pas: parser_ancestors(cx.ancestors, node) };
        let src = cx.src;
        let loc = node.location();
        // (offense range, found definitions)
        let mut range = (loc.start_offset(), loc.end_offset());
        let mut found: Vec<Found> = Vec::new();
        match kind::of(node) {
            kind::DEF_NODE => {
                if ctx.inside_if() {
                    return;
                }
                let def = node.as_def_node().unwrap();
                let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
                range = (def.def_keyword_loc().start_offset(), def.name_loc().end_offset());
                match def.receiver() {
                    None => found.extend(ctx.found_instance_method(&name, src)),
                    Some(r) if is_const(&r) => {
                        if let Some(short) = short_const(&r) {
                            // lookup_constant without a match: `each_ancestor` returns the defs
                            // node itself, so the key is its s-expression (#360).
                            found.extend(ctx.found_const_method(&short, &name).or_else(|| {
                                let sexp = helpers::parser_sexp::sexp(node, src)?;
                                Some(ctx.found_method(format!("{sexp}.{name}"), None))
                            }));
                        }
                    }
                    Some(r) if r.as_self_node().is_some() => found.extend(ctx.found_self_method(&name, src)),
                    _ => {}
                }
            }
            kind::ALIAS_METHOD_NODE => {
                let a = node.as_alias_method_node().unwrap();
                let (Some(name), Some(orig)) = (sym_value(&a.new_name()), sym_value(&a.old_name())) else { return };
                if name == orig || ctx.inside_if() {
                    return;
                }
                found.extend(ctx.found_instance_method(&name, src));
            }
            _ => {
                let call = node.as_call_node().unwrap();
                if call.receiver().is_some() || is_csend(&call) {
                    return;
                }
                range = send_range(&call);
                let args = send_args(&call);
                let method = call.name();
                match method.as_slice() {
                    b"alias_method" => {
                        if args.len() != 2 {
                            return;
                        }
                        let (Some(name), Some(orig)) = (sym_value(&args[0]), sym_value(&args[1])) else { return };
                        if name == orig || ctx.inside_if() {
                            return;
                        }
                        found.extend(ctx.found_instance_method(&name, src));
                    }
                    m @ (b"attr_reader" | b"attr_writer" | b"attr_accessor" | b"attr") if !args.is_empty() => {
                        let (readable, writable, list): (bool, bool, &[Node]) = match m {
                            b"attr" => {
                                let w = args.len() == 2 && args[1].as_true_node().is_some();
                                (true, w, &args[..1])
                            }
                            b"attr_reader" => (true, false, &args),
                            b"attr_writer" => (false, true, &args),
                            _ => (true, true, &args),
                        };
                        for a in list {
                            let Some(name) = sym_value(a) else { continue };
                            if readable {
                                found.extend(ctx.found_instance_method(&name, src));
                            }
                            if writable {
                                found.extend(ctx.found_instance_method(&format!("{name}="), src));
                            }
                        }
                    }
                    b"def_delegator" | b"def_instance_delegator" => {
                        let all_str_sym = args.iter().all(|a| str_or_sym_value(a).is_some());
                        let name = match args.len() {
                            2 | 3 if all_str_sym => str_or_sym_value(args.last().unwrap()),
                            _ => None,
                        };
                        let Some(name) = name else { return };
                        if ctx.inside_if() {
                            return;
                        }
                        found.extend(ctx.found_instance_method(&name, src));
                    }
                    b"def_delegators" | b"def_instance_delegators" => {
                        if args.len() < 2 || !args.iter().all(|a| str_or_sym_value(a).is_some()) || ctx.inside_if() {
                            return;
                        }
                        for a in &args[1..] {
                            found.extend(ctx.found_instance_method(&str_or_sym_value(a).unwrap(), src));
                        }
                    }
                    m if cx.config.all_cops().truthy("ActiveSupportExtensionsEnabled")
                        && cx.cfg.strings("DelegatingMethods").iter().any(|d| d.as_bytes() == m) =>
                    {
                        let Some(names) = delegate_names(&args) else { return };
                        if ctx.inside_if() {
                            return;
                        }
                        for name in names {
                            found.extend(ctx.found_instance_method(&name, src));
                        }
                    }
                    _ => return,
                }
            }
        }
        let line_of = |off: usize| src.line_of(off);
        let path = smart_path(cx.src.path);
        for f in found {
            let st = cx.state::<State>();
            let Some(&prev) = st.definitions.get(&f.key) else {
                st.definitions.insert(f.key, loc.start_offset());
                continue;
            };
            let scoped = match f.handler {
                Some(PT::Rescue) => Some(&mut st.rescue),
                Some(PT::Ensure) => Some(&mut st.ensure),
                _ => None,
            };
            if let Some(set) = scoped
                && !set.contains(&f.key)
            {
                set.insert(f.key.clone());
                st.definitions.insert(f.key, loc.start_offset());
                continue;
            }
            let msg = format!(
                "Method `{}` is defined at both {path}:{} and {path}:{}.",
                f.method,
                line_of(prev),
                line_of(loc.start_offset())
            );
            cx.add(range.0, range.1, msg);
        }
    }
}

/// `delegate_args` plus `delegate_prefix`: names `delegate :a, :b, to: :x, prefix: ...` defines.
fn delegate_names(args: &[Node]) -> Option<Vec<String>> {
    let (last, names) = args.split_last()?;
    let hash = last.as_keyword_hash_node().map(|h| h.elements()).or_else(|| last.as_hash_node().map(|h| h.elements()))?;
    if names.is_empty() || !names.iter().all(|n| str_or_sym_value(n).is_some()) {
        return None;
    }
    let pair = |key: &str| {
        hash.iter().find_map(|e| {
            let a = e.as_assoc_node()?;
            (sym_value(&a.key()).as_deref() == Some(key)).then(|| a.value())
        })
    };
    let to = pair("to").filter(|v| str_or_sym_value(v).is_some())?;
    let prefix = match pair("prefix") {
        Some(p) if p.as_true_node().is_some() => str_or_sym_value(&to),
        Some(p) => str_or_sym_value(&p),
        None => None,
    };
    Some(
        names
            .iter()
            .map(|n| {
                let n = str_or_sym_value(n).unwrap();
                match &prefix {
                    Some(p) => format!("{p}_{n}"),
                    None => n,
                }
            })
            .collect(),
    )
}
