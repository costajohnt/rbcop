use crate::prelude::*;

pub const NAME: &str = "Lint/UselessRuby2Keywords";
pub static COP: UselessRuby2Keywords = UselessRuby2Keywords;
pub struct UselessRuby2Keywords;

/// `allowed_arguments?`: a `*rest` and no keyword parameters. `params` is `None` when there are
/// no parameters at all.
fn allowed(params: Option<&ruby_prism::ParametersNode>) -> bool {
    let Some(p) = params else { return false };
    p.rest().is_some_and(|r| r.as_rest_parameter_node().is_some())
        && p.keywords().iter().next().is_none()
        && !p.keyword_rest().is_some_and(|k| k.as_keyword_rest_parameter_node().is_some())
}

/// A definition of `name` among a parser node's children: `(def name ...)` or
/// `(any_block (send _ :define_method (sym name)) ...)`; the result says whether its arguments
/// are allowed.
fn definition(node: &Node, name: &[u8]) -> Option<bool> {
    if let Some(d) = node.as_def_node() {
        return (d.receiver().is_none() && d.name().as_slice() == name).then(|| allowed(d.parameters().as_ref()));
    }
    let call = node.as_call_node()?;
    let block = call.block()?.as_block_node()?;
    let args = call_args(&call);
    let [sym] = args.as_slice() else { return None };
    if method_name(&call) != "define_method" || sym.as_symbol_node()?.unescaped() != name {
        return None;
    }
    // Numbered/`it` blocks have no arguments.
    let params = block.parameters();
    let bp = params.as_ref().and_then(|p| p.as_block_parameters_node());
    Some(allowed(bp.as_ref().and_then(|b| b.parameters()).as_ref()))
}

impl Cop for UselessRuby2Keywords {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "ruby2_keywords" {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        if let Some(def) = first.as_def_node().filter(|d| d.receiver().is_none()) {
            if allowed(def.parameters().as_ref()) {
                return;
            }
            let Some(sel) = call.message_loc() else { return };
            let name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
            cx.add(sel.start_offset(), sel.end_offset(), format!("`ruby2_keywords` is unnecessary for method `{name}`."));
            return;
        }
        let Some(sym) = first.as_symbol_node() else { return };
        // `node.parent`: a lone top-level statement has none.
        if is_lone_root(cx.ancestors) {
            return;
        }
        let name = sym.unescaped().to_vec();
        // `find_method_definition`: the nearest ancestor with a matching child definition,
        // stopping at the enclosing class, module or sclass.
        let mut found = None;
        for a in cx.ancestors.iter().rev() {
            let children: Vec<Node> = match kind::of(a) {
                kind::STATEMENTS_NODE => a.as_statements_node().unwrap().body().iter().collect(),
                kind::ARGUMENTS_NODE => a.as_arguments_node().unwrap().arguments().iter().collect(),
                _ => vec![],
            };
            found = children.iter().find_map(|c| definition(c, &name));
            if found.is_some() || matches!(kind::of(a), kind::CLASS_NODE | kind::MODULE_NODE | kind::SINGLETON_CLASS_NODE) {
                break;
            }
        }
        if found != Some(false) {
            return;
        }
        let (start, end) = send_range(&call);
        let name = String::from_utf8_lossy(&name).into_owned();
        cx.add(start, end, format!("`ruby2_keywords` is unnecessary for method `{name}`."));
    }
}

/// The node is the program's only statement (no parser parent).
fn is_lone_root(ancestors: &[Node]) -> bool {
    ancestors.len() == 2 && ancestors[1].as_statements_node().is_some_and(|s| s.body().iter().count() == 1)
}
