use crate::cops::sorbet_enforce_signatures::is_signature;
use crate::cops::sorbet_obsolete_strict_memoization::t_send;
use crate::helpers::sorbet_t_let::{replace_t_let, statically_scoped};
use crate::helpers::sorbet_target_version::enabled_for;
use crate::prelude::*;

pub const NAME: &str = "Sorbet/RedundantTLet";
pub static COP: RedundantTLet = RedundantTLet;
pub struct RedundantTLet;

const MSG: &str = "Unnecessary T.let. The instance variable type is inferred from the signature.";
const MSG_CONSTRUCTOR: &str = "Unnecessary T.let. The constant type is inferred from the constructor.";
const GENERIC_CLASSES: &[&str] = &["Array", "Class", "Enumerator", "Hash", "Module", "Range", "Set"];

/// A def's parameters in source order as (parser type, name), for `named_argument_types`.
/// (parser type, name, range) of a def parameter.
pub type DefParam<'pr> = (&'static str, Option<&'pr [u8]>, (usize, usize));

pub fn def_params<'pr>(params: Option<ruby_prism::ParametersNode<'pr>>) -> Vec<DefParam<'pr>> {
    let Some(p) = params else { return Vec::new() };
    let mut out = Vec::new();
    let mut add = |n: Node<'pr>| {
        let loc = n.location();
        let r = (loc.start_offset(), loc.end_offset());
        let item = match &n {
            Node::RequiredParameterNode { .. } => ("arg", Some(n.as_required_parameter_node().unwrap().name().as_slice())),
            Node::MultiTargetNode { .. } => ("mlhs", None),
            Node::OptionalParameterNode { .. } => ("optarg", Some(n.as_optional_parameter_node().unwrap().name().as_slice())),
            Node::RestParameterNode { .. } => ("restarg", n.as_rest_parameter_node().unwrap().name().map(|c| c.as_slice())),
            Node::RequiredKeywordParameterNode { .. } => {
                ("kwarg", Some(n.as_required_keyword_parameter_node().unwrap().name().as_slice()))
            }
            Node::OptionalKeywordParameterNode { .. } => {
                ("kwoptarg", Some(n.as_optional_keyword_parameter_node().unwrap().name().as_slice()))
            }
            Node::KeywordRestParameterNode { .. } => {
                ("kwrestarg", n.as_keyword_rest_parameter_node().unwrap().name().map(|c| c.as_slice()))
            }
            Node::NoKeywordsParameterNode { .. } => ("kwnilarg", None),
            Node::ForwardingParameterNode { .. } => ("forward_arg", None),
            Node::BlockParameterNode { .. } => ("blockarg", n.as_block_parameter_node().unwrap().name().map(|c| c.as_slice())),
            _ => return,
        };
        out.push((item.0, item.1, r));
    };
    p.requireds().iter().for_each(&mut add);
    p.optionals().iter().for_each(&mut add);
    if let Some(r) = p.rest() {
        add(r);
    }
    p.posts().iter().for_each(&mut add);
    p.keywords().iter().for_each(&mut add);
    if let Some(r) = p.keyword_rest() {
        add(r);
    }
    if let Some(b) = p.block() {
        add(b.as_node());
    }
    out.sort_by_key(|x| x.2.0);
    out
}

/// RedundantTLet#normalize_whitespace.
fn normalize_whitespace(s: &str) -> String {
    static RES: LazyLock<[(Regex, &str); 7]> = LazyLock::new(|| {
        [
            (Regex::new(r"[ \t\r\n\x0b\x0c]+").unwrap(), " "),
            (Regex::new(r",[ \t\r\n\x0b\x0c]*([)\]}])").unwrap(), "$1"),
            (Regex::new(r",[ \t\r\n\x0b\x0c]*").unwrap(), ", "),
            (Regex::new(r"\([ \t\r\n\x0b\x0c]*").unwrap(), "("),
            (Regex::new(r"[ \t\r\n\x0b\x0c]*\)").unwrap(), ")"),
            (Regex::new(r"\[[ \t\r\n\x0b\x0c]*").unwrap(), "["),
            (Regex::new(r"[ \t\r\n\x0b\x0c]*\]").unwrap(), "]"),
        ]
    });
    let mut s = s.to_string();
    for (re, to) in RES.iter() {
        s = re.replace_all(&s, *to).into_owned();
    }
    helpers::ruby_string::strip(&s).to_string()
}

/// "`(send nil? :params (hash $...))": the first `params(<hash>)` call in pre-order.
fn sig_params<'pr>(sig: &Node<'pr>) -> Option<Vec<Node<'pr>>> {
    struct V<'pr>(Option<Vec<Node<'pr>>>);
    impl<'pr> ruby_prism::Visit<'pr> for V<'pr> {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            if self.0.is_some() {
                return;
            }
            if !is_csend(node)
                && node.receiver().is_none()
                && method_name(node) == "params"
                && node.block().is_none_or(|b| b.as_block_node().is_none())
            {
                let args = call_args(node);
                if node.block().is_none()
                    && let [h] = args.as_slice()
                {
                    let elements = h.as_hash_node().map(|h| h.elements()).or_else(|| h.as_keyword_hash_node().map(|h| h.elements()));
                    if let Some(e) = elements {
                        self.0 = Some(e.iter().collect());
                        return;
                    }
                }
            }
            ruby_prism::visit_call_node(self, node);
        }
    }
    let mut v = V(None);
    ruby_prism::Visit::visit(&mut v, sig);
    v.0
}

impl RedundantTLet {
    fn on_def(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() || def.name().as_slice() != b"initialize" {
            return;
        }
        let args: Vec<(&str, &[u8])> = def_params(def.parameters())
            .into_iter()
            .filter(|a| a.0 != "mlhs")
            .map(|(k, n, _)| (k, n.unwrap_or(b"\0")))
            .collect();
        if args.is_empty() {
            return;
        }
        // find_sig_node: not under a modifier send, the left sibling statement is a signature.
        let anc = cx.ancestors;
        let Some(pi) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return };
        if anc[pi].as_call_node().is_some_and(|c| !is_csend(&c)) {
            return;
        }
        let Some(stmts) = anc[pi].as_statements_node() else { return };
        let stmts: Vec<Node> = stmts.body().iter().collect();
        let start = node.location().start_offset();
        let Some(i) = stmts.iter().position(|s| s.location().start_offset() == start) else { return };
        if i == 0 || !is_signature(&stmts[i - 1]) {
            return;
        }
        let Some(pairs) = sig_params(&stmts[i - 1]) else { return };
        // `pair.key.value`: a symbol key's name; a `**` or interpolated-symbol key raises.
        let mut sig: Vec<(Option<Vec<u8>>, Node)> = Vec::new();
        for p in pairs {
            let Some(a) = p.as_assoc_node() else {
                return crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting());
            };
            let key = a.key();
            if key.as_interpolated_symbol_node().is_some() {
                return crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting());
            }
            let name: Option<Vec<u8>> = key.as_symbol_node().map(|s| s.unescaped().to_vec());
            // `.to_h`: the last pair for a key wins.
            sig.retain(|(k, _)| name.is_none() || *k != name);
            sig.push((name, a.value()));
        }
        if sig.is_empty() {
            return;
        }
        let body = def.body();
        let ivasgns: Vec<Node> = match body.as_ref().and_then(|b| b.as_statements_node()) {
            Some(s) => s.body().iter().filter(|n| n.as_instance_variable_write_node().is_some()).collect(),
            None => Vec::new(),
        };
        for w in ivasgns {
            let value = w.as_instance_variable_write_node().unwrap().value();
            let Some((_, a)) = t_send(&value, "let", 2) else { continue };
            let Some(lvar) = a[0].as_local_variable_read_node() else { continue };
            let key = lvar.name().as_slice();
            let Some((_, sig_type)) = sig.iter().rev().find(|(k, _)| k.as_deref() == Some(key)) else { continue };
            let Some(&(kind, _)) = args.iter().rev().find(|(_, n)| *n == key) else { continue };
            let t = normalize_whitespace(node_text(sig_type));
            let expected = match kind {
                "restarg" => format!("T::Array[{t}]"),
                "kwrestarg" => format!("T::Hash[Symbol, {t}]"),
                _ => t,
            };
            if normalize_whitespace(node_text(&a[1])) != expected {
                continue;
            }
            let loc = value.location();
            let (s, e) = (loc.start_offset(), loc.end_offset());
            let name = String::from_utf8_lossy(key).into_owned();
            cx.add_fix(s, e, MSG, |c| c.replace(s, e, &name));
        }
    }

    fn on_casgn(&self, cx: &mut Cx, value: Node) {
        match enabled_for("0.6.13304") {
            Ok(true) => {}
            Ok(false) => return,
            Err(_) => return crate::runner::cop_error_at(NAME, cx.src, crate::runner::visiting()),
        }
        if !statically_scoped(cx.ancestors) {
            return;
        }
        let Some((_, a)) = t_send(&value, "let", 2) else { return };
        let ty = &a[1];
        if !matches!(ty, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }) {
            return;
        }
        // constructor_call: through a trailing `.freeze` and a block, a `Const.new` send.
        let mut v = a[0].as_call_node();
        if let Some(c) = &v
            && !is_csend(c)
            && c.block().is_none_or(|b| b.as_block_node().is_none())
            && method_name(c) == "freeze"
        {
            v = c.receiver().and_then(|r| r.as_call_node());
            if a[0].as_call_node().unwrap().receiver().is_none() {
                return;
            }
        }
        let Some(ctor) = v.filter(|c| !is_csend(c) && method_name(c) == "new") else { return };
        let Some(recv) = ctor.receiver().filter(|r| matches!(r, Node::ConstantReadNode { .. } | Node::ConstantPathNode { .. }))
        else {
            return;
        };
        let class_path = node_text(&recv).strip_prefix("::").unwrap_or(node_text(&recv));
        if GENERIC_CLASSES.contains(&class_path) || class_path != node_text(ty).strip_prefix("::").unwrap_or(node_text(ty)) {
            return;
        }
        let loc = value.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let (rs, re, text) = replace_t_let(cx.src, (s, e), &a[0]);
        cx.add_fix(s, e, MSG_CONSTRUCTOR, |c| c.replace(rs, re, &text));
    }
}

impl Cop for RedundantTLet {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE, kind::CONSTANT_WRITE_NODE, kind::CONSTANT_PATH_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::DefNode { .. } => self.on_def(cx, node),
            Node::ConstantWriteNode { .. } => self.on_casgn(cx, node.as_constant_write_node().unwrap().value()),
            _ => self.on_casgn(cx, node.as_constant_path_write_node().unwrap().value()),
        }
    }
}
