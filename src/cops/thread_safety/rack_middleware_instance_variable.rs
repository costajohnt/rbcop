use crate::helpers::configurable_naming::allowed_identifier;
use crate::helpers::node_types::lvar_name;
use crate::helpers::thread_safety::{each_node, is_def, operation_produces_threadsafe_object};
use crate::prelude::*;
use ruby_prism::{DefNode, ParametersNode};

pub const NAME: &str = "ThreadSafety/RackMiddlewareInstanceVariable";
pub static COP: RackMiddlewareInstanceVariable = RackMiddlewareInstanceVariable;
pub struct RackMiddlewareInstanceVariable;

const MSG: &str = "Avoid instance variables in Rack middleware.";

/// `(args (arg _)+)`: only required positional parameters (no destructuring), `n` of them or, with
/// None, at least one.
fn plain_args(params: Option<ParametersNode>, n: Option<usize>) -> bool {
    let Some(p) = params else { return false };
    let req: Vec<Node> = p.requireds().iter().collect();
    !req.is_empty()
        && n.is_none_or(|n| req.len() == n)
        && req.iter().all(|r| r.as_required_parameter_node().is_some())
        && p.optionals().iter().next().is_none()
        && p.rest().is_none()
        && p.posts().iter().next().is_none()
        && p.keywords().iter().next().is_none()
        && p.keyword_rest().is_none()
        && p.block().is_none()
}

/// `def_node.arguments.size`
fn arg_count(def: &DefNode) -> usize {
    def.parameters().map_or(0, |p| {
        p.requireds().iter().count()
            + p.optionals().iter().count()
            + usize::from(p.rest().is_some_and(|r| r.as_implicit_rest_node().is_none()))
            + p.posts().iter().count()
            + p.keywords().iter().count()
            + usize::from(p.keyword_rest().is_some())
            + usize::from(p.block().is_some())
    })
}

fn named(def: &DefNode, name: &str) -> bool {
    def.name().as_slice() == name.as_bytes()
}

/// rack_middleware_like_class?
fn middleware_like(class: &ruby_prism::ClassNode) -> bool {
    if class.constant_path().as_constant_read_node().is_none() || class.superclass().is_some() {
        return false;
    }
    let Some(stmts) = class.body().and_then(|b| b.as_statements_node()) else { return false };
    let defs: Vec<DefNode> = stmts.body().iter().filter_map(|n| n.as_def_node()).filter(|d| d.receiver().is_none()).collect();
    stmts.body().iter().count() >= 2
        && defs.iter().any(|d| named(d, "initialize") && plain_args(d.parameters(), None))
        && defs.iter().any(|d| named(d, "call") && plain_args(d.parameters(), Some(1)))
}

/// A parser `ivasgn`/`ivar`: its name and source range.
fn ivar<'pr>(node: &Node<'pr>) -> Option<(&'pr [u8], (usize, usize))> {
    let r = |l: ruby_prism::Location| (l.start_offset(), l.end_offset());
    Some(match node {
        Node::InstanceVariableReadNode { .. } => (node.as_instance_variable_read_node().unwrap().name().as_slice(), r(node.location())),
        Node::InstanceVariableWriteNode { .. } => (node.as_instance_variable_write_node().unwrap().name().as_slice(), r(node.location())),
        Node::InstanceVariableTargetNode { .. } => (node.as_instance_variable_target_node().unwrap().name().as_slice(), r(node.location())),
        Node::InstanceVariableOrWriteNode { .. } => {
            let n = node.as_instance_variable_or_write_node().unwrap();
            (n.name().as_slice(), r(n.name_loc()))
        }
        Node::InstanceVariableAndWriteNode { .. } => {
            let n = node.as_instance_variable_and_write_node().unwrap();
            (n.name().as_slice(), r(n.name_loc()))
        }
        Node::InstanceVariableOperatorWriteNode { .. } => {
            let n = node.as_instance_variable_operator_write_node().unwrap();
            (n.name().as_slice(), r(n.name_loc()))
        }
        _ => return None,
    })
}

impl RackMiddlewareInstanceVariable {
    fn on_class(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        if !middleware_like(&class) {
            return;
        }
        let all = each_node(node);
        let Some(ctor) = all
            .iter()
            .filter_map(|n| n.as_def_node())
            .find(|d| d.receiver().is_none() && named(d, "initialize") && arg_count(d) >= 1)
        else {
            return;
        };
        // app_variable: `(def :initialize (args (arg $_) ...) `(ivasgn $_ (lvar $_)))`, the ivasgn name.
        // ponytail: only the constructor itself is matched, not a `def initialize` nested in it.
        if !ctor.parameters().and_then(|p| p.requireds().iter().next()).is_some_and(|r| r.as_required_parameter_node().is_some()) {
            return;
        }
        let ctor_nodes = each_node(&ctor.as_node());
        let writes = || ctor_nodes.iter().filter_map(|n| n.as_instance_variable_write_node());
        let Some(app) = writes().find(|w| lvar_name(&w.value()).is_some()).map(|w| w.name().as_slice()) else { return };
        let safe: Vec<&[u8]> =
            writes().filter(|w| operation_produces_threadsafe_object(&w.value())).map(|w| w.name().as_slice()).collect();
        let mut found = Vec::new();
        for def in all.iter().filter(|n| is_def(n)) {
            for n in each_node(def) {
                let Some((name, range)) = ivar(&n) else { continue };
                if name == app || safe.contains(&name) || allowed_identifier(cx.cfg, &String::from_utf8_lossy(name)) {
                    continue;
                }
                found.push(range);
            }
        }
        for (s, e) in found {
            cx.add(s, e, MSG);
        }
    }
}

impl Cop for RackMiddlewareInstanceVariable {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = node.as_call_node() else { return self.on_class(cx, node) };
        if !matches!(method_name(&call), "instance_variable_get" | "instance_variable_set") {
            return;
        }
        let Some(arg) = send_args(&call).into_iter().next() else { return };
        let value = match &arg {
            Node::SymbolNode { .. } if !is_parser_dsym(&arg, cx.config.target_ruby) => arg.as_symbol_node().unwrap().unescaped().to_vec(),
            Node::StringNode { .. } if !is_parser_dstr(&arg) => arg.as_string_node().unwrap().unescaped().to_vec(),
            _ => return,
        };
        if allowed_identifier(cx.cfg, &String::from_utf8_lossy(&value)) {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, MSG);
    }
}
