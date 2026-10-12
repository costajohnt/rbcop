use crate::prelude::*;
use ruby_prism::CallNode;

pub const NAME: &str = "Lint/Debugger";
pub static COP: Debugger = Debugger;
pub struct Debugger;

/// Whether any entry of a flattened DebuggerMethods/DebuggerRequires value (an Array, or a Hash of
/// groups, `nil` groups dropped) satisfies `f`. Runs per call node, so it borrows instead of
/// collecting the list.
fn any_entry(v: Option<&Value>, f: &mut dyn FnMut(&str) -> bool) -> bool {
    match v {
        Some(Value::Seq(items)) => items.iter().any(|i| any_entry(Some(i), f)),
        Some(Value::Map(m)) => m.0.iter().any(|(_, v)| any_entry(Some(v), f)),
        Some(Value::Str(s) | Value::Sym(s) | Value::Regex(s)) => f(s),
        Some(v) => v.as_string().is_some_and(|s| f(&s)),
        None => false,
    }
}

/// Lint/Debugger#chained_method_name, e.g. `Kernel.binding.irb`.
fn chained_name(node: &CallNode) -> String {
    let mut name = method_name(node).to_string();
    let mut recv = node.receiver();
    while let Some(r) = recv {
        let (part, next) = if let Some(c) = r.as_call_node() {
            (method_name(&c).to_string(), c.receiver())
        } else if r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some() {
            (node_text(&r).trim_start_matches("::").to_string(), None)
        } else {
            (String::new(), None)
        };
        name = format!("{part}.{name}");
        recv = next;
    }
    name
}

impl Cop for Debugger {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let name = method_name(&call);
        let args = call_args(&call);
        let hit = if name == "require" {
            args.len() == 1
                && args[0].as_string_node().is_some_and(|s| {
                    any_entry(cx.cfg.get("DebuggerRequires"), &mut |r| r.as_bytes() == s.unescaped())
                })
        } else {
            let methods = cx.cfg.get("DebuggerMethods");
            any_entry(methods, &mut |m| m.ends_with(name) && m.rsplit('.').next() == Some(name)) && {
                let chained = chained_name(&call);
                any_entry(methods, &mut |m| m == chained)
            }
        };
        if !hit {
            return;
        }
        // Lint/Debugger#assumed_usage_context?
        let anc = parser_ancestors(cx.ancestors);
        if args.is_empty() && anc.contains(&Frame::Call) {
            if matches!(anc.first(), Some(Frame::Call | Frame::Literal | Frame::Pair)) {
                return;
            }
            if !anc.iter().any(|f| matches!(f, Frame::Block | Frame::KwBegin)) {
                return;
            }
        }
        let (start, end) = send_range(&call);
        let msg = format!("Remove debugger entry point `{}`.", cx.src.slice(start, end));
        cx.add(start, end, msg);
    }
}
