use crate::prelude::*;
use std::collections::HashSet;

pub const NAME: &str = "Lint/ConstantReassignment";
pub static COP: ConstantReassignment = ConstantReassignment;
pub struct ConstantReassignment;

impl Cop for ConstantReassignment {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CLASS_NODE,
            kind::MODULE_NODE,
            kind::CONSTANT_WRITE_NODE,
            kind::CONSTANT_PATH_WRITE_NODE,
            kind::CONSTANT_TARGET_NODE,
            kind::CONSTANT_PATH_TARGET_NODE,
            kind::CALL_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        match node {
            Node::ClassNode { .. } | Node::ModuleNode { .. } => {
                if !cx.ancestors.iter().all(|a| {
                    matches!(
                        kind::of(a),
                        kind::PROGRAM_NODE
                            | kind::STATEMENTS_NODE
                            | kind::PARENTHESES_NODE
                            | kind::CLASS_NODE
                            | kind::MODULE_NODE
                    )
                }) {
                    return;
                }
                let mut ns = ancestor_namespaces(cx.ancestors);
                append_namespaces(&mut ns, &identifier(node));
                let constant = ns.pop().unwrap_or_default();
                let name = qualified(&ns, &constant);
                cx.state::<HashSet<String>>().insert(name);
            }
            Node::CallNode { .. } => {
                let call = node.as_call_node().unwrap();
                let Some(constant) = remove_constant(&call) else { return };
                let ns = ancestor_namespaces(cx.ancestors);
                if ns.is_empty() {
                    return;
                }
                let name = qualified(&ns, &constant);
                cx.state::<HashSet<String>>().remove(&name);
            }
            _ => on_casgn(cx, node),
        }
    }
}

/// `(send {nil? self} :remove_const ({sym str} $_))`
fn remove_constant(call: &ruby_prism::CallNode) -> Option<String> {
    if method_name(call) != "remove_const" || is_csend(call) || call.block().is_some() {
        return None;
    }
    if !call.receiver().is_none_or(|r| r.as_self_node().is_some()) {
        return None;
    }
    let [arg] = send_args(call).try_into().ok()?;
    let bytes = match &arg {
        Node::SymbolNode { .. } => arg.as_symbol_node().unwrap().unescaped().to_vec(),
        Node::StringNode { .. } if !is_parser_dstr(&arg) => arg.as_string_node().unwrap().unescaped().to_vec(),
        _ => return None,
    };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

enum Ns<'pr> {
    Nil,
    Cbase,
    Node(Node<'pr>),
}

#[derive(PartialEq)]
enum Seg {
    Cbase,
    Const(String),
    SelfNode,
    Other,
}

/// A const node: its namespace and short name.
struct Const<'pr> {
    ns: Ns<'pr>,
    name: String,
}

fn name_of(id: &ruby_prism::ConstantId) -> String {
    String::from_utf8_lossy(id.as_slice()).into_owned()
}

fn path_ns(parent: Option<Node>) -> Ns {
    parent.map_or(Ns::Cbase, Ns::Node)
}

fn as_const<'pr>(node: &Node<'pr>) -> Option<Const<'pr>> {
    match node {
        Node::ConstantReadNode { .. } => Some(Const { ns: Ns::Nil, name: name_of(&node.as_constant_read_node().unwrap().name()) }),
        Node::ConstantPathNode { .. } => {
            let p = node.as_constant_path_node().unwrap();
            Some(Const { ns: path_ns(p.parent()), name: p.name().map(|n| name_of(&n)).unwrap_or_default() })
        }
        _ => None,
    }
}

/// The casgn-like view of an assignment target.
fn casgn<'pr>(node: &Node<'pr>) -> Option<Const<'pr>> {
    match node {
        Node::ConstantWriteNode { .. } => Some(Const { ns: Ns::Nil, name: name_of(&node.as_constant_write_node().unwrap().name()) }),
        Node::ConstantTargetNode { .. } => Some(Const { ns: Ns::Nil, name: name_of(&node.as_constant_target_node().unwrap().name()) }),
        Node::ConstantPathWriteNode { .. } => as_const(&node.as_constant_path_write_node().unwrap().target().as_node()),
        Node::ConstantPathTargetNode { .. } => {
            let t = node.as_constant_path_target_node().unwrap();
            Some(Const { ns: path_ns(t.parent()), name: t.name().map(|n| name_of(&n)).unwrap_or_default() })
        }
        _ => None,
    }
}

/// `each_path`: the namespace chain, outermost first.
fn each_path(ns: &Ns) -> Vec<Seg> {
    match ns {
        Ns::Nil => Vec::new(),
        Ns::Cbase => vec![Seg::Cbase],
        Ns::Node(n) => match as_const(n) {
            Some(c) => {
                let mut out = each_path(&c.ns);
                out.push(Seg::Const(c.name));
                out
            }
            None if n.as_self_node().is_some() => vec![Seg::SelfNode],
            None => vec![Seg::Other],
        },
    }
}

fn const_names(path: &[Seg]) -> impl Iterator<Item = String> + '_ {
    path.iter().filter_map(|s| match s {
        Seg::Const(n) => Some(n.clone()),
        _ => None,
    })
}

fn absolute(c: &Const) -> bool {
    !matches!(c.ns, Ns::Nil) && each_path(&c.ns).first() == Some(&Seg::Cbase)
}

fn identifier<'pr>(node: &Node<'pr>) -> Const<'pr> {
    let path = match node {
        Node::ClassNode { .. } => node.as_class_node().unwrap().constant_path(),
        _ => node.as_module_node().unwrap().constant_path(),
    };
    as_const(&path).unwrap_or(Const { ns: Ns::Nil, name: String::new() })
}

fn append_namespaces(ns: &mut Vec<String>, id: &Const) {
    if absolute(id) {
        ns.clear();
    }
    ns.extend(const_names(&each_path(&id.ns)));
    ns.push(id.name.clone());
}

fn ancestor_namespaces(ancestors: &[Node]) -> Vec<String> {
    let mut ns = Vec::new();
    for a in ancestors.iter().filter(|a| matches!(a, Node::ClassNode { .. } | Node::ModuleNode { .. })) {
        append_namespaces(&mut ns, &identifier(a));
    }
    ns
}

fn qualified(ns: &[String], constant: &str) -> String {
    let mut parts = vec![""];
    parts.extend(ns.iter().map(String::as_str));
    parts.push(constant);
    parts.join("::")
}

fn on_casgn(cx: &mut Cx, node: &Node) {
    let Some(c) = casgn(node) else { return };
    let path = each_path(&c.ns);
    if !path.iter().all(|s| *s != Seg::Other) || !simple_assignment(cx.ancestors) {
        return;
    }
    let name = if absolute(&c) {
        let ns = match &c.ns {
            Ns::Node(n) if as_const(n).is_some() => node_text(n),
            _ => "",
        };
        format!("{ns}::{}", c.name)
    } else {
        let mut ns = ancestor_namespaces(cx.ancestors);
        ns.extend(const_names(&path));
        qualified(&ns, &c.name)
    };
    let mut display: Vec<String> = const_names(&path).collect();
    display.push(c.name.clone());
    let display = display.join("::");
    let loc = node.location();
    if cx.state::<HashSet<String>>().contains(&name) {
        let msg = format!("Constant `{display}` is already assigned in this namespace.");
        cx.add(loc.start_offset(), loc.end_offset(), msg);
        return;
    }
    // report_cross_file_collision: with the project index, the first definition of the constant
    // in another file.
    let prior = crate::project_index::get().and_then(|index| {
        let ix = index.read();
        let decl = ix.lookup(name.strip_prefix("::").unwrap_or(&name))?;
        let (_, path, _) = ix.in_other_files(&ix.definitions(decl), &cx.src.absolute_path()).into_iter().next()?;
        Some(path)
    });
    cx.state::<HashSet<String>>().insert(name);
    if let Some(path) = prior {
        let msg = format!("Constant `{display}` is already assigned in `{}`.", path.display());
        cx.add(loc.start_offset(), loc.end_offset(), msg);
    }
}

/// `simple_assignment?`: every parser ancestor up to the nearest class/module is a begin, literal,
/// casgn, masgn/mlhs or `freeze` send.
fn simple_assignment(ancestors: &[Node]) -> bool {
    for a in ancestors.iter().rev() {
        match a {
            Node::ClassNode { .. } | Node::ModuleNode { .. } => return true,
            Node::ProgramNode { .. }
            | Node::StatementsNode { .. }
            | Node::ArgumentsNode { .. }
            | Node::ParenthesesNode { .. }
            | Node::EmbeddedStatementsNode { .. }
            | Node::ConstantWriteNode { .. }
            | Node::ConstantPathWriteNode { .. }
            | Node::MultiWriteNode { .. }
            | Node::MultiTargetNode { .. } => {}
            Node::CallNode { .. } => {
                let call = a.as_call_node().unwrap();
                if method_name(&call) != "freeze" || is_csend(&call) || call.block().is_some_and(|b| b.as_block_node().is_some()) {
                    return false;
                }
            }
            _ if helpers::literal_node::literal(a) => {}
            _ => return false,
        }
    }
    true
}
