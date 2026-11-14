use crate::helpers::sorbet_rbi_type::{Error, rbs_of};
use crate::helpers::whole_lines::range_by_whole_lines;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTStruct";
pub static COP: ForbidTStruct = ForbidTStruct;
pub struct ForbidTStruct;

const MSG_STRUCT: &str = "Using `T::Struct` or its variants is deprecated in this codebase.";
const MSG_PROPS: &str = "Using `T::Props` or its variants is deprecated in this codebase.";

/// `(const (const {nil? cbase} :T) name)`
fn t_const(node: &Node, names: &[&str]) -> bool {
    node.as_constant_path_node().is_some_and(|p| {
        p.name().is_some_and(|n| names.iter().any(|x| x.as_bytes() == n.as_slice()))
            && p.parent().is_some_and(|q| helpers::parser_parent::is_top_const(&q, "T"))
    })
}

/// A parser `send` (no csend, no literal block).
fn send<'pr>(node: &Node<'pr>) -> Option<ruby_prism::CallNode<'pr>> {
    node.as_call_node().filter(|c| !is_csend(c) && c.block().is_none_or(|b| b.as_block_node().is_none()))
}

struct Prop {
    start: usize,
    end: usize,
    reader: bool,
    name: String,
    raw_type: String,
    default: Option<String>,
    factory: Option<String>,
}

impl Prop {
    /// Property#type: whitespace removed, a space after each comma.
    fn ty(&self) -> String {
        self.raw_type.chars().filter(|c| !c.is_whitespace()).collect::<String>().replace(',', ", ")
    }
    fn nilable(&self) -> bool {
        self.ty().starts_with("T.nilable(")
    }
    fn optional(&self) -> bool {
        self.default.is_some() || self.factory.is_some() || self.nilable()
    }
    fn rbs_type(&self) -> Result<String, Error> {
        match rbs_of(&self.raw_type) {
            Err(Error::Type) => Ok("untyped".into()),
            r => r,
        }
    }
    fn sig_param(&self, rbs: bool) -> Result<String, Error> {
        let t = if rbs { self.rbs_type()? } else { self.ty() };
        Ok(if rbs && self.optional() { format!("?{}: {t}", self.name) } else { format!("{}: {t}", self.name) })
    }
    fn param(&self) -> String {
        let mut s = format!("{}:", self.name);
        if let Some(d) = self.default.as_ref().or(self.factory.as_ref()) {
            s.push_str(&format!(" {d}"));
        } else if self.nilable() {
            s.push_str(" nil");
        }
        s
    }
}

/// TStructWalker: the class body's props and its `extend T::Sig`, not entering nested scopes nor
/// the arguments of any send.
#[derive(Default)]
struct Walker {
    props: Vec<Prop>,
    extend_t_sig: Option<(usize, usize)>,
    has_extend_t_sig: bool,
    /// A prop the corrector raises on (missing arguments, default and factory both given).
    raises: bool,
}

impl<'pr> ruby_prism::Visit<'pr> for Walker {
    fn visit_class_node(&mut self, _: &ruby_prism::ClassNode<'pr>) {}
    fn visit_module_node(&mut self, _: &ruby_prism::ModuleNode<'pr>) {}
    fn visit_singleton_class_node(&mut self, _: &ruby_prism::SingletonClassNode<'pr>) {}
    fn visit_def_node(&mut self, _: &ruby_prism::DefNode<'pr>) {}

    fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
        if is_csend(node) {
            return ruby_prism::visit_call_node(self, node);
        }
        self.on_send(node);
        // Parser visits a call's block as the `block` parent, the send first: its body is walked.
        if let Some(b) = node.block().and_then(|b| b.as_block_node()) {
            ruby_prism::Visit::visit_block_node(self, &b);
        }
    }
}

impl Walker {
    fn on_send(&mut self, node: &ruby_prism::CallNode) {
        let args = call_args(node);
        let name = method_name(node);
        if name == "extend" && args.len() == 1 && node.block().is_none_or(|b| b.as_block_node().is_some())
            && t_const(&args[0], &["Sig"]) {
            let l = node.location();
            self.has_extend_t_sig = true;
            self.extend_t_sig = Some((l.start_offset(), send_range(node).1));
            return;
        }
        if node.receiver().is_some() || !matches!(name, "const" | "prop") {
            return;
        }
        let mut all = args;
        if let Some(b) = node.block().filter(|b| b.as_block_argument_node().is_some()) {
            all.push(b);
        }
        let (Some(first), Some(second)) = (all.first(), all.get(1)) else {
            self.raises = true;
            return;
        };
        let (mut default, mut factory) = (None, None);
        for a in all.iter().skip(2) {
            let elements = a.as_hash_node().map(|h| h.elements()).or_else(|| a.as_keyword_hash_node().map(|h| h.elements()));
            for e in elements.iter().flat_map(|e| e.iter()) {
                let Some(pair) = e.as_assoc_node() else { continue };
                let key = pair.key();
                // Parser's label `sym` range excludes the colon.
                let key = node_text(&key).trim_end_matches(':');
                match key {
                    "default" => default = Some(node_text(&pair.value()).to_string()),
                    "factory" => factory = Some(node_text(&pair.value()).to_string()),
                    _ => {}
                }
            }
        }
        if default.is_some() && factory.is_some() {
            self.raises = true;
            return;
        }
        let (start, end) = send_range(node);
        self.props.push(Prop {
            start,
            end,
            reader: name == "const",
            name: node_text(first).strip_prefix(':').unwrap_or(node_text(first)).to_string(),
            raw_type: node_text(second).to_string(),
            default,
            factory,
        });
    }
}

impl Cop for ForbidTStruct {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node.as_class_node() {
            Some(class) => self.on_class(cx, node, &class),
            None => self.on_send(cx, node),
        }
    }
}

impl ForbidTStruct {
    fn on_send(&self, cx: &mut Cx, node: &Node) {
        let Some(c) = send(node) else { return };
        if c.receiver().is_some() || !matches!(method_name(&c), "include" | "prepend" | "extend") {
            return;
        }
        let args = call_args(&c);
        let [arg] = args.as_slice() else { return };
        if c.block().is_some() {
            return;
        }
        struct Find(bool);
        impl<'pr> ruby_prism::Visit<'pr> for Find {
            fn visit_constant_path_node(&mut self, node: &ruby_prism::ConstantPathNode<'pr>) {
                if t_const(&node.as_node(), &["Props"]) {
                    self.0 = true;
                }
                ruby_prism::visit_constant_path_node(self, node);
            }
        }
        let mut f = Find(false);
        ruby_prism::Visit::visit(&mut f, arg);
        if f.0 {
            let (s, e) = send_range(&c);
            cx.add_with(s, e, MSG_PROPS, false);
        }
    }

    fn on_class(&self, cx: &mut Cx, node: &Node, class: &ruby_prism::ClassNode) {
        let Some(superclass) = class.superclass() else { return };
        if !t_const(&superclass, &["Struct", "ImmutableStruct", "InexactStruct"]) {
            return;
        }
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        match self.correction(cx, node, class, &superclass) {
            Ok(edits) => cx.add_fix(s, e, MSG_STRUCT, |c| {
                for (kind, s, e, t) in &edits {
                    match kind {
                        'r' => c.replace(*s, *e, t),
                        'x' => c.remove(*s, *e),
                        _ => c.insert_after(*s, *e, t),
                    }
                }
            }),
            Err(()) => crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting()),
        }
    }

    #[allow(clippy::type_complexity)]
    fn correction(
        &self,
        cx: &Cx,
        node: &Node,
        class: &ruby_prism::ClassNode,
        superclass: &Node,
    ) -> Result<Vec<(char, usize, usize, String)>, ()> {
        let style = cx.cfg.str("AutocorrectStyle").unwrap_or("sig");
        if !matches!(style, "sig" | "rbs") {
            return Err(());
        }
        let rbs = style == "rbs";
        let src = cx.src;
        let text = src.text;
        let mut w = Walker::default();
        if let Some(body) = class.body() {
            ruby_prism::Visit::visit(&mut w, &body);
        }
        if w.raises {
            return Err(());
        }
        let ident_end = class.constant_path().location().end_offset();
        let mut edits = vec![('x', ident_end, superclass.location().end_offset(), String::new())];
        let loc = node.location();
        if src.line_of(loc.start_offset()) == src.line_of(loc.end_offset()) {
            return Ok(edits);
        }
        if rbs && let Some((s, e)) = w.extend_t_sig {
            let (s, e) = range_by_whole_lines(text, s, e, true);
            edits.push(('x', s, e, String::new()));
        }
        let class_indent = " ".repeat(src.column(loc.start_offset()));
        if !w.has_extend_t_sig && !rbs {
            edits.push(('i', ident_end, ident_end, format!("\n{class_indent}  extend T::Sig\n")));
        }
        for (i, p) in w.props.iter().enumerate() {
            let indent = " ".repeat(src.column(p.start));
            let (ls, le) = range_by_whole_lines(text, p.start, p.end, false);
            let line = src.line_of(p.start);
            let prev_blank = line >= 2 && {
                let prev = src.line_start(line - 1);
                text[prev..src.line_start(line) - 1].chars().all(char::is_whitespace)
            };
            let new_line = i != 0 && !prev_blank;
            let comments: String = src
                .comments
                .iter()
                .filter(|c| src.line_of(c.start) == line)
                .map(|c| format!("{indent}{}\n", src.slice(c.start, c.end)))
                .collect();
            let sig = if rbs { format!("#: {}", p.rbs_type().map_err(|_| ())?) } else { format!("sig {{ returns({}) }}", p.ty()) };
            let kind = if p.reader { "attr_reader" } else { "attr_accessor" };
            edits.push((
                'r',
                ls,
                le,
                format!("{}{comments}{indent}{sig}\n{indent}{kind} :{}", if new_line { "\n" } else { "" }, p.name),
            ));
        }
        if let Some(last) = w.props.last() {
            let indent = " ".repeat(src.column(last.start));
            let (_, le) = range_by_whole_lines(text, last.start, last.end, true);
            let init = self.initialize_method(cx, &indent, &w.props, rbs).map_err(|_| ())?;
            edits.push(('i', le, le, init));
        }
        Ok(edits)
    }

    fn initialize_method(&self, cx: &Cx, indent: &str, props: &[Prop], rbs: bool) -> Result<String, Error> {
        // ponytail: a stable sort; Ruby's sort_by is unstable above 16 elements, so a struct with
        // more than 16 props mixing optional and required ones may order them differently.
        let mut sorted: Vec<&Prop> = props.iter().collect();
        sorted.sort_by_key(|p| p.optional());
        let max = helpers::statement_modifier::max_line_length(cx);
        let fits = |line: &str| max.is_none_or(|m| line.chars().count() as f64 <= m);
        let params: Vec<String> = sorted.iter().map(|p| p.sig_param(rbs)).collect::<Result<_, _>>()?;
        let mut s = String::from("\n");
        if rbs {
            s.push_str(&format!("{indent}#: ({}) -> void\n", params.join(", ")));
        } else {
            let line = format!("{indent}sig {{ params({}).void }}\n", params.join(", "));
            if fits(&line) {
                s.push_str(&line);
            } else {
                s.push_str(&format!("{indent}sig do\n{indent}  params(\n"));
                for (i, p) in params.iter().enumerate() {
                    s.push_str(&format!("{indent}    {p}{}\n", if i + 1 < params.len() { "," } else { "" }));
                }
                s.push_str(&format!("{indent}  ).void\n{indent}end\n"));
            }
        }
        let ps: Vec<String> = sorted.iter().map(|p| p.param()).collect();
        let line = format!("{indent}def initialize({})\n", ps.join(", "));
        if fits(&line) {
            s.push_str(&line);
        } else {
            s.push_str(&format!("{indent}def initialize(\n"));
            for (i, p) in ps.iter().enumerate() {
                s.push_str(&format!("{indent}  {p}{}\n", if i + 1 < ps.len() { "," } else { "" }));
            }
            s.push_str(&format!("{indent})\n"));
        }
        for p in props {
            s.push_str(&format!("{indent}  @{} = {}{}\n", p.name, p.name, if p.factory.is_some() { ".call" } else { "" }));
        }
        s.push_str(&format!("{indent}end\n"));
        Ok(s)
    }
}
