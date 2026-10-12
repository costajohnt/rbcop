use crate::prelude::*;

pub const NAME: &str = "Rails/Delegate";
pub static COP: Delegate = Delegate;
pub struct Delegate;

/// full_const_name
fn full_const_name(n: &Node) -> Option<String> {
    if let Some(c) = n.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let p = n.as_constant_path_node()?;
    let short = String::from_utf8_lossy(p.name()?.as_slice()).into_owned();
    Some(match p.parent() {
        None => format!("::{short}"),
        Some(ns) => format!("{}::{short}", full_const_name(&ns).unwrap_or_default()),
    })
}

enum Recv {
    SelfNode,
    SelfClass,
    Send(String),
    Var(String),
    Const(String),
}

impl Cop for Delegate {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        if def.receiver().is_some() {
            return;
        }
        let stmts = helpers::rails::body_statements(def.body());
        let [body] = stmts.as_slice() else { return };
        // A body with rescue/ensure is not a send.
        let Some(send) = helpers::rails::send_node(body) else { return };
        let Some(r) = send.receiver() else { return };
        let recv = if r.as_self_node().is_some() {
            Recv::SelfNode
        } else if let Some(rc) = helpers::rails::send_node(&r) {
            if rc.receiver().is_none() && send_args(&rc).is_empty() {
                Recv::Send(method_name(&rc).to_string())
            } else if method_name(&rc) == "class" && rc.receiver().is_some_and(|x| x.as_self_node().is_some()) && send_args(&rc).is_empty() {
                Recv::SelfClass
            } else {
                return;
            }
        } else if matches!(r, Node::InstanceVariableReadNode { .. } | Node::ClassVariableReadNode { .. } | Node::GlobalVariableReadNode { .. }) {
            Recv::Var(node_text(&r).to_string())
        } else if r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some() {
            Recv::Const(full_const_name(&r).unwrap_or_default())
        } else {
            return;
        };
        let body_name = method_name(&send);
        let def_name = String::from_utf8_lossy(def.name().as_slice()).into_owned();
        let prefixed = match &recv {
            Recv::SelfNode => String::new(),
            Recv::Var(v) | Recv::Const(v) | Recv::Send(v) => format!("{v}_{body_name}"),
            Recv::SelfClass => format!("class_{body_name}"),
        };
        let enforce_prefixed = cx.cfg.get("EnforceForPrefixed").is_some_and(|v| !matches!(v, Value::Null | Value::Bool(false)));
        if def_name != body_name && !(enforce_prefixed && def_name == prefixed) {
            return;
        }
        // arguments_match?: plain required args passed through as lvars, in order.
        let params: Vec<Node> = def.parameters().map(|p| parameter_nodes(&p)).unwrap_or_default();
        let args = send_args(&send);
        if params.len() != args.len() {
            return;
        }
        let ok = params.iter().zip(&args).all(|(p, a)| {
            p.as_required_parameter_node()
                .zip(a.as_local_variable_read_node())
                .is_some_and(|(p, a)| p.name().as_slice() == a.name().as_slice())
        });
        if !ok {
            return;
        }
        // private_or_protected_inline?
        let src = cx.src;
        let line = src.lines[src.line_of(node.location().start_offset()) - 1];
        let stripped = helpers::ruby_string::strip(line);
        if stripped.starts_with("private ") || stripped.contains("protected ") {
            return;
        }
        let anc = cx.ancestors;
        let parent = helpers::rails::parent_index(anc).map(|i| &anc[i]);
        let (siblings, idx) = helpers::visibility_help::siblings(anc, node);
        if helpers::visibility_help::node_visibility(node, parent, &siblings, idx) != "public" {
            return;
        }
        // module_function_declared?: a module or `begin` ancestor with a `module_function` send child.
        let module_function = |n: &Node| helpers::rails::send_node(n).is_some_and(|c| method_name(&c) == "module_function");
        let declared = anc.iter().any(|a| {
            if let Some(m) = a.as_module_node() {
                let stmts = helpers::rails::body_statements(m.body());
                return stmts.len() == 1 && module_function(&stmts[0]);
            }
            a.as_statements_node().is_some_and(|s| s.body().iter().count() > 1 && s.body().iter().any(|c| module_function(&c)))
        });
        if declared {
            return;
        }
        let receiver = match &recv {
            Recv::SelfNode => "self".to_string(),
            Recv::Const(c) if c.contains("::") => format!(":'{c}'"),
            Recv::Const(c) | Recv::Var(c) | Recv::Send(c) => format!(":{c}"),
            Recv::SelfClass => ":class".to_string(),
        };
        let mut delegation = format!("delegate :{body_name}, to: {receiver}");
        if !prefixed.is_empty() && def_name == prefixed {
            delegation += ", prefix: true";
        }
        let kw = def.def_keyword_loc();
        let (s, e) = (node.location().start_offset(), node.location().end_offset());
        cx.add_fix(kw.start_offset(), kw.end_offset(), "Use `delegate` to define delegations.", |c| {
            c.replace(s, e, &delegation)
        });
    }
}
