use crate::prelude::*;

pub const NAME: &str = "Style/EmptyClassDefinition";
pub static COP: EmptyClassDefinition = EmptyClassDefinition;
pub struct EmptyClassDefinition;

/// rubocop-ast `children.empty?` for a class body node.
fn childless(node: &Node) -> bool {
    match kind::of(node) {
        kind::NIL_NODE
        | kind::TRUE_NODE
        | kind::FALSE_NODE
        | kind::SELF_NODE
        | kind::FORWARDING_SUPER_NODE
        | kind::REDO_NODE
        | kind::RETRY_NODE => true,
        kind::RETURN_NODE => node.as_return_node().unwrap().arguments().is_none(),
        kind::BREAK_NODE => node.as_break_node().unwrap().arguments().is_none(),
        kind::NEXT_NODE => node.as_next_node().unwrap().arguments().is_none(),
        kind::YIELD_NODE => node.as_yield_node().unwrap().arguments().is_none(),
        kind::SUPER_NODE => {
            let s = node.as_super_node().unwrap();
            s.arguments().is_none() && s.block().is_none()
        }
        kind::ARRAY_NODE => node.as_array_node().unwrap().elements().iter().next().is_none(),
        kind::HASH_NODE => node.as_hash_node().unwrap().elements().iter().next().is_none(),
        kind::PARENTHESES_NODE => node.as_parentheses_node().unwrap().body().is_none(),
        kind::BEGIN_NODE => {
            let b = node.as_begin_node().unwrap();
            b.statements().is_none() && b.rescue_clause().is_none() && b.else_clause().is_none() && b.ensure_clause().is_none()
        }
        // An empty heredoc is a childless `dstr`.
        kind::STRING_NODE | kind::INTERPOLATED_STRING_NODE => {
            is_heredoc(node)
                && node.as_string_node().is_some_and(|s| s.content_loc().start_offset() == s.content_loc().end_offset())
        }
        _ => false,
    }
}

impl Cop for EmptyClassDefinition {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE, kind::CONSTANT_WRITE_NODE, kind::CONSTANT_PATH_WRITE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("");
        let allowed = cx.cfg.strings("AllowedParentClasses");
        if let Some(class) = node.as_class_node() {
            if style != "class_new" {
                return;
            }
            let Some(parent) = class.superclass() else { return };
            if class.body().is_some_and(|b| match b.as_statements_node() {
                Some(st) => {
                    let list: Vec<Node> = st.body().iter().collect();
                    !(list.len() == 1 && childless(&list[0]))
                }
                None => true,
            }) {
                return;
            }
            let parent_src = node_text(&parent);
            if allowed.iter().any(|a| a == parent_src) {
                return;
            }
            let replacement = format!("{} = Class.new({parent_src})", node_text(&class.constant_path()));
            let loc = node.location();
            let (s, e) = (loc.start_offset(), loc.end_offset());
            let msg = "Use `Class.new` instead of the `class` keyword to define an empty class.";
            cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
            return;
        }
        if style != "class_keyword" && style != "class_definition" {
            return;
        }
        // `(casgn _ _ $(send (const _ :Class) :new _))`
        let (value, name) = if let Some(w) = node.as_constant_write_node() {
            (w.value(), String::from_utf8_lossy(w.name().as_slice()).into_owned())
        } else {
            let w = node.as_constant_path_write_node().unwrap();
            let target = w.target();
            let name = target.name().map(|n| String::from_utf8_lossy(n.as_slice()).into_owned()).unwrap_or_default();
            let full = match target.parent() {
                Some(ns) => format!("{}::{name}", node_text(&ns)),
                // A cbase namespace's source is `::`.
                None => format!("::::{name}"),
            };
            (w.value(), full)
        };
        let Some(call) = value.as_call_node() else { return };
        if is_csend(&call) || call.block().is_some() || method_name(&call) != "new" {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        let is_class_const = recv.as_constant_read_node().is_some_and(|c| c.name().as_slice() == b"Class")
            || recv
                .as_constant_path_node()
                .is_some_and(|c| c.name().is_some_and(|n| n.as_slice() == b"Class"));
        if !is_class_const {
            return;
        }
        let args = call_args(&call);
        let [arg] = args.as_slice() else { return };
        if arg.as_constant_read_node().is_none() && arg.as_constant_path_node().is_none() {
            return;
        }
        let parent_src = node_text(arg);
        if allowed.iter().any(|a| a == parent_src) {
            return;
        }
        let loc = node.location();
        let (s, e) = (loc.start_offset(), loc.end_offset());
        let indent = " ".repeat(cx.src.column(s));
        let replacement = format!("class {name} < {parent_src}\n{indent}end");
        let msg = "Use the `class` keyword instead of `Class.new` to define an empty class.";
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &replacement));
    }
}
