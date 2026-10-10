use crate::helpers::node_equality::same;
use crate::prelude::*;
use ruby_prism::{CallNode, Visit};

pub const NAME: &str = "Style/HashEachMethods";
pub static COP: HashEachMethods = HashEachMethods;
pub struct HashEachMethods;

const ARRAY_CONVERTER_METHODS: &[&str] = &["assoc", "chunk", "flatten", "rassoc", "sort", "sort_by", "to_a"];

fn range(node: &Node) -> (usize, usize) {
    let l = node.location();
    (l.start_offset(), l.end_offset())
}

/// rubocop-ast `receiver` on any node: only calls (and blocks, via their call) have one.
fn parser_receiver<'pr>(node: &Node<'pr>) -> Option<Node<'pr>> {
    node.as_call_node()?.receiver()
}

fn is_const(node: &Node) -> bool {
    matches!(kind::of(node), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE)
}

/// A call without an attached `{}`/`do` block (a parser `send`/`csend`, not a `block`).
fn plain_call<'pr>(node: &Node<'pr>) -> Option<CallNode<'pr>> {
    node.as_call_node().filter(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
}

/// AllowedReceivers#receiver_name.
fn receiver_name(node: &Node) -> String {
    if let Some(r) = parser_receiver(node)
        && !is_const(&r)
    {
        return receiver_name(&r);
    }
    if let Some(call) = plain_call(node).filter(|c| !is_csend(c)) {
        return match call.receiver() {
            Some(r) => format!("{}.{}", receiver_name(&r), method_name(&call)),
            None => method_name(&call).to_string(),
        };
    }
    node_text(node).to_string()
}

fn allowed_receiver(cx: &Cx, node: &Node) -> bool {
    let name = receiver_name(node);
    cx.cfg.strings("AllowedReceivers").contains(&name)
}

/// `(call _ ${:keys :values})` with no arguments: the method name.
fn keys_or_values<'pr>(node: &Node<'pr>) -> Option<(CallNode<'pr>, &'pr str)> {
    let call = plain_call(node)?;
    let name = method_name(&call);
    (matches!(name, "keys" | "values") && call.arguments().is_none() && call.block().is_none()).then_some((call, name))
}

/// `` `(send %1 :[]= ...) ``
struct Mutation<'a, 's> {
    src: &'a crate::source::Source<'s>,
    root: (usize, usize),
    found: bool,
}

impl<'pr> Visit<'pr> for Mutation<'_, '_> {
    fn visit_call_node(&mut self, node: &CallNode<'pr>) {
        if method_name(node) == "[]="
            && !is_csend(node)
            && node.receiver().is_some_and(|r| same(self.src, range(&r), self.root))
        {
            self.found = true;
        }
        ruby_prism::visit_call_node(self, node);
    }
}

/// Sources of `lvar` descendants of a block body.
#[derive(Default)]
struct Lvars(Vec<String>);

impl<'pr> Visit<'pr> for Lvars {
    fn visit_local_variable_read_node(&mut self, node: &ruby_prism::LocalVariableReadNode<'pr>) {
        self.0.push(String::from_utf8_lossy(node.name().as_slice()).into_owned());
    }
    fn visit_it_local_variable_read_node(&mut self, _node: &ruby_prism::ItLocalVariableReadNode<'pr>) {
        self.0.push("it".to_string());
    }
}

/// Sources of the `arg`/`restarg` descendants of an mlhs block parameter (`*` stripped).
fn mlhs_names(node: &Node, out: &mut Vec<String>) {
    if let Some(m) = node.as_multi_target_node() {
        for n in m.lefts().iter().chain(crate::ast::parser_rest(m.rest())).chain(m.rights().iter()) {
            mlhs_names(&n, out);
        }
    } else if let Some(s) = node.as_splat_node() {
        if let Some(e) = s.expression() {
            mlhs_names(&e, out);
        }
    } else if node.as_required_parameter_node().is_some() {
        out.push(node_text(node).to_string());
    }
}

fn unused(lvars: &[String], arg: &Node) -> bool {
    if arg.as_multi_target_node().is_some() {
        let mut names = Vec::new();
        mlhs_names(arg, &mut names);
        names.iter().all(|n| !lvars.contains(n))
    } else {
        let s = node_text(arg);
        let s = s.strip_prefix('*').unwrap_or(s);
        !lvars.iter().any(|l| l == s)
    }
}

impl Cop for HashEachMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::BLOCK_NODE, kind::BLOCK_ARGUMENT_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = cx.ancestors.last().and_then(|p| p.as_call_node()) else { return };
        if node.as_block_argument_node().is_some() {
            return self.on_block_pass(cx, node, &call);
        }
        let block = node.as_block_node().unwrap();
        let src = cx.src;
        let call_node = call.as_node();
        // handleable?
        if let Some(r) = call.receiver()
            && r.as_call_node().is_some_and(|c| ARRAY_CONVERTER_METHODS.contains(&method_name(&c)))
        {
            return;
        }
        let Some(mut root) = call.receiver() else { return };
        while let Some(r) = parser_receiver(&root) {
            root = r;
        }
        let mut m = Mutation { src, root: range(&root), found: false };
        m.visit(&call_node);
        if m.found || (helpers::literal_node::literal(&root) && root.as_hash_node().is_none()) {
            return;
        }

        // kv_each
        if method_name(&call) == "each"
            && call.arguments().is_none()
            && let Some((target_recv, kv)) = call.receiver().as_ref().and_then(keys_or_values)
            && let Some(parent_receiver) = target_recv.receiver()
            && !allowed_receiver(cx, &parent_receiver)
        {
            let sel = target_recv.message_loc().unwrap();
            let (_, target_end) = send_range(&call);
            let current = &src.text[sel.start_offset()..target_end];
            let prefer = format!("each_{}", &kv[..kv.len() - 1]);
            let msg = format!("Use `{prefer}` instead of `{current}`.");
            let each_sel_end = call.message_loc().unwrap().end_offset();
            let start = call.location().start_offset();
            let dot = call.call_operator_loc().map_or("", |d| text(&d));
            let new_source = format!("{}{dot}{prefer}", node_text(&parent_receiver));
            let before = cx.out.len();
            cx.add_fix(sel.start_offset(), each_sel_end, msg, |c| c.replace(start, target_end, &new_source));
            if cx.out.len() > before {
                return;
            }
        }

        // each_arguments: (block (call _ :each) (args $_key $_value) ...)
        if method_name(&call) != "each" || call.arguments().is_some() {
            return;
        }
        let Some(params) = block.parameters().and_then(|p| p.as_block_parameters_node()) else { return };
        if params.locals().iter().count() != 0 {
            return;
        }
        let Some(p) = params.parameters() else { return };
        let args = parameter_nodes(&p);
        let [key, value] = args.as_slice() else { return };
        let Some(body) = block.body() else { return };
        let mut lvars = Lvars::default();
        match body.as_statements_node() {
            Some(s) if s.body().iter().count() == 1 => {
                let only = s.body().iter().next().unwrap();
                if only.as_local_variable_read_node().is_none() && only.as_it_local_variable_read_node().is_none() {
                    lvars.visit(&only);
                }
            }
            _ => lvars.visit(&body),
        }
        let value_unused = unused(&lvars.0, value);
        let key_unused = unused(&lvars.0, key);
        if value_unused && key_unused {
            return;
        }
        let (prefer, unused_code, unused_range) = if value_unused {
            ("each_key", node_text(value), (range(key).1, range(value).1))
        } else if key_unused {
            ("each_value", node_text(key), (range(key).0, range(value).0))
        } else {
            return;
        };
        let msg = format!("Use `{prefer}` instead of `each` and remove the unused `{unused_code}` block argument.");
        let sel = call.message_loc().unwrap();
        let (start, end) = (call.location().start_offset(), node.location().end_offset());
        cx.add_fix(start, end, msg, |c| {
            c.replace(sel.start_offset(), sel.end_offset(), prefer);
            c.remove(unused_range.0, unused_range.1);
        });
    }
}

impl HashEachMethods {
    /// on_block_pass: `(call $(call _ ${:keys :values}) :each (block_pass (sym _)))`.
    fn on_block_pass(&self, cx: &mut Cx, node: &Node, call: &CallNode) {
        let bp = node.as_block_argument_node().unwrap();
        if method_name(call) != "each"
            || call.arguments().is_some()
            || bp.expression().is_none_or(|e| e.as_symbol_node().is_none())
        {
            return;
        }
        let Some((target, kv)) = call.receiver().as_ref().and_then(keys_or_values) else { return };
        let Some(parent_receiver) = target.receiver() else { return };
        if allowed_receiver(cx, &parent_receiver) {
            return;
        }
        let (start, end) = (target.message_loc().unwrap().start_offset(), call.message_loc().unwrap().end_offset());
        let current = &cx.src.text[start..end];
        let prefer = format!("each_{}", &kv[..kv.len() - 1]);
        let msg = format!("Use `{prefer}` instead of `{current}`.");
        cx.add_fix(start, end, msg, |c| c.replace(start, end, &prefer));
    }
}
