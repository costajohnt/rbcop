use crate::helpers::macro_scope::is_macro;
use crate::helpers::thread_safety::{self as ts, Anc};
use crate::prelude::*;

pub const NAME: &str = "ThreadSafety/ClassInstanceVariable";
pub static COP: ClassInstanceVariable = ClassInstanceVariable;
pub struct ClassInstanceVariable;

const MSG: &str = "Avoid class instance variables.";

const ACTION_CALLBACKS: &[&str] = &[
    "prepend_around_action",
    "prepend_before_action",
    "before_action",
    "append_before_action",
    "around_action",
    "append_around_action",
    "append_after_action",
    "after_action",
    "prepend_after_action",
];

/// new_lexical_scope?: `Struct.new`/`Class.new`/`Data.define` blocks whose body is one method
/// definition, and receiver-less action callbacks without arguments taking a block.
fn new_lexical_scope(a: &Anc) -> bool {
    let Anc::Block { body, plain: true, .. } = a else { return false };
    let Some(call) = a.block_call().filter(|c| !is_csend(c)) else { return false };
    let name = method_name(&call);
    let Some(recv) = call.receiver() else {
        return ACTION_CALLBACKS.contains(&name) && send_args(&call).is_empty();
    };
    let constructor = recv.as_constant_read_node().is_some_and(|c| match c.name().as_slice() {
        b"Struct" | b"Class" => name == "new",
        b"Data" => name == "define",
        _ => false,
    });
    constructor
        && body.as_ref().and_then(|b| b.as_statements_node()).is_some_and(|s| {
            let v: Vec<Node> = s.body().iter().collect();
            matches!(v.as_slice(), [d] if d.as_def_node().is_some())
        })
}

/// `node.ancestors` up to (not including) the first new lexical scope, or None when one is met
/// before `stop` matches (`break` in the gem's `find`/`any?` blocks).
fn before_scope<'a, 'b, 'pr>(ancs: &'b [Anc<'a, 'pr>]) -> impl Iterator<Item = (usize, &'b Anc<'a, 'pr>)> {
    ancs.iter().enumerate().take_while(|(_, a)| !new_lexical_scope(a))
}

fn is_def(a: &Anc) -> bool {
    a.node().is_some_and(ts::is_def)
}

/// The nearest receiver-less def before a new lexical scope (its index in `ancs`).
fn enclosing_def(ancs: &[Anc]) -> Option<usize> {
    before_scope(ancs).find(|(_, a)| is_def(a)).map(|(i, _)| i)
}

fn in_defs(ancs: &[Anc]) -> bool {
    before_scope(ancs).any(|(_, a)| a.node().and_then(|n| n.as_def_node()).is_some_and(|d| d.receiver().is_some()))
}

fn in_def_sclass(ancs: &[Anc]) -> bool {
    enclosing_def(ancs).is_some_and(|i| ancs[i + 1..].iter().any(|a| matches!(a.node(), Some(Node::SingletonClassNode { .. }))))
}

fn in_def_class_methods(ancs: &[Anc]) -> bool {
    let dsl = before_scope(ancs).any(|(_, a)| {
        a.is_block() && a.block_call().is_some_and(|c| c.receiver().is_none() && method_name(&c) == "class_methods")
    });
    dsl || enclosing_def(ancs).is_some_and(|i| {
        ancs[i + 1..]
            .iter()
            .find_map(|a| a.node().filter(|n| matches!(n, Node::ClassNode { .. } | Node::ModuleNode { .. })))
            .and_then(|m| m.as_module_node())
            .is_some_and(|m| {
                let c = m.constant_path();
                let short = match &c {
                    Node::ConstantReadNode { .. } => Some(c.as_constant_read_node().unwrap().name()),
                    Node::ConstantPathNode { .. } => c.as_constant_path_node().unwrap().name(),
                    _ => None,
                };
                short.is_some_and(|n| n.as_slice() == b"ClassMethods")
            })
    })
}

/// in_def_module_function?: the nearest def (any depth) has a bare `module_function` before it,
/// or `module_function :name` after it, among its siblings.
fn in_def_module_function(ancestors: &[Node]) -> bool {
    let Some(k) = ancestors.iter().rposition(ts::is_def) else { return false };
    let def = ancestors[k].as_def_node().unwrap();
    // parser siblings: the other statements of a `begin` (several statements).
    let Some(stmts) = k.checked_sub(1).and_then(|p| ancestors[p].as_statements_node()) else { return false };
    let body: Vec<Node> = stmts.body().iter().collect();
    if body.len() < 2 {
        return false;
    }
    let start = def.location().start_offset();
    let Some(pos) = body.iter().position(|n| n.location().start_offset() == start) else { return false };
    let scope = &ancestors[..k];
    let left = body[..pos].iter().any(|n| {
        n.as_call_node().is_some_and(|c| {
            !is_csend(&c)
                && c.block().is_none()
                && method_name(&c) == "module_function"
                && c.arguments().is_none()
                && is_macro(&c, scope)
        })
    });
    let name = def.name().as_slice();
    left || body[pos + 1..].iter().any(|n| {
        n.as_call_node().is_some_and(|c| {
            if is_csend(&c) || c.receiver().is_some() || method_name(&c) != "module_function" || c.block().is_some() {
                return false;
            }
            let args = call_args(&c);
            let [arg] = args.as_slice() else { return false };
            match arg {
                Node::SymbolNode { .. } => arg.as_symbol_node().unwrap().unescaped() == name,
                Node::StringNode { .. } if !is_parser_dstr(arg) => arg.as_string_node().unwrap().unescaped() == name,
                _ => false,
            }
        })
    })
}

/// in_class_eval?: the nearest block (before a def or a new lexical scope) is
/// `(block (send (const {nil? cbase} _) {:class_eval :class_exec}) ...)`.
fn in_class_eval(ancs: &[Anc]) -> bool {
    for a in ancs {
        if is_def(a) || new_lexical_scope(a) {
            return false;
        }
        if a.is_block() {
            return a.block_call().is_some_and(|c| {
                !is_csend(&c)
                    && matches!(method_name(&c), "class_eval" | "class_exec")
                    && c.arguments().is_none()
                    && c.receiver().is_some_and(|r| {
                        r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some_and(|p| p.parent().is_none())
                    })
            });
        }
    }
    false
}

/// An ancestor (before a new lexical scope) whose first child is the command `name`.
fn in_command(ancs: &[Anc], name: &str) -> bool {
    before_scope(ancs).any(|(_, a)| a.first_send().is_some_and(|c| c.receiver().is_none() && method_name(&c) == name))
}

/// synchronized?: a `block` ancestor (any depth) of a `send` whose last child is `:synchronize`.
fn synchronized(ancs: &[Anc]) -> bool {
    ancs.iter().any(|a| {
        a.is_block() && a.block_call().is_some_and(|c| !is_csend(&c) && method_name(&c) == "synchronize" && call_args(&c).is_empty())
    })
}

fn flagged(ancestors: &[Node]) -> bool {
    let ancs = ts::parser_ancestors(ancestors);
    let class_method = in_defs(&ancs)
        || in_def_sclass(&ancs)
        || in_def_class_methods(&ancs)
        || in_def_module_function(ancestors)
        || in_class_eval(&ancs)
        || in_command(&ancs, "define_singleton_method");
    class_method && !in_command(&ancs, "define_method") && !synchronized(&ancs)
}

impl Cop for ClassInstanceVariable {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::INSTANCE_VARIABLE_READ_NODE,
            kind::INSTANCE_VARIABLE_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
            kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
            kind::INSTANCE_VARIABLE_TARGET_NODE,
            kind::CALL_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // on_ivar / on_ivasgn: the variable name; on_send: instance_variable_set/get calls.
        let (s, e) = match node {
            Node::InstanceVariableWriteNode { .. } => {
                let l = node.as_instance_variable_write_node().unwrap().name_loc();
                (l.start_offset(), l.end_offset())
            }
            Node::InstanceVariableOrWriteNode { .. } => {
                let l = node.as_instance_variable_or_write_node().unwrap().name_loc();
                (l.start_offset(), l.end_offset())
            }
            Node::InstanceVariableAndWriteNode { .. } => {
                let l = node.as_instance_variable_and_write_node().unwrap().name_loc();
                (l.start_offset(), l.end_offset())
            }
            Node::InstanceVariableOperatorWriteNode { .. } => {
                let l = node.as_instance_variable_operator_write_node().unwrap().name_loc();
                (l.start_offset(), l.end_offset())
            }
            Node::CallNode { .. } => {
                // (send nil? :instance_variable_set (...) (...)) / (send nil? :instance_variable_get (...))
                let call = node.as_call_node().unwrap();
                let want = match method_name(&call) {
                    "instance_variable_set" => 2,
                    "instance_variable_get" => 1,
                    _ => return,
                };
                if call.receiver().is_some() || send_args(&call).len() != want {
                    return;
                }
                send_range(&call)
            }
            _ => {
                let l = node.location();
                (l.start_offset(), l.end_offset())
            }
        };
        if flagged(cx.ancestors) {
            cx.add(s, e, MSG);
        }
    }
}
