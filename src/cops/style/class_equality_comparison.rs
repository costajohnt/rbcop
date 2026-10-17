use crate::prelude::*;

pub const NAME: &str = "Style/ClassEqualityComparison";
pub static COP: ClassEqualityComparison = ClassEqualityComparison;
pub struct ClassEqualityComparison;

const CLASS_NAME_METHODS: &[&str] = &["name", "to_s", "inspect"];

/// `(send _ :name)` with no arguments (a plain send, not csend).
fn plain_send<'pr>(node: &Node<'pr>, name: &str) -> Option<ruby_prism::CallNode<'pr>> {
    let call = node.as_call_node()?;
    let ok = !is_csend(&call)
        && method_name(&call) == name
        && call.arguments().is_none()
        && call.block().is_none();
    ok.then_some(call)
}

impl Cop for ClassEqualityComparison {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || !matches!(method_name(&call), "==" | "equal?" | "eql?") {
            return;
        }
        if let Some(def) = cx.ancestors.iter().rev().find_map(|a| a.as_def_node()) {
            let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
            if helpers::allowed::allowed_method(cx.cfg, name) || helpers::allowed::matches_allowed_pattern(cx.cfg, name) {
                return;
            }
        }
        let args = call_args(&call);
        if args.len() != 1 || call.block().is_some() {
            return;
        }
        let Some(recv) = call.receiver() else { return };
        // {$(send _ :class) (send $(send _ :class) #class_name_method?)}
        let (class_send, name_method) = if let Some(c) = plain_send(&recv, "class") {
            (c, false)
        } else {
            let Some(outer) = recv.as_call_node() else { return };
            if is_csend(&outer) || outer.arguments().is_some() || outer.block().is_some() {
                return;
            }
            if !CLASS_NAME_METHODS.contains(&method_name(&outer)) {
                return;
            }
            let Some(inner) = outer.receiver().and_then(|r| plain_send(&r, "class")) else { return };
            (inner, true)
        };
        let class_node = &args[0];
        if is_parser_dstr(class_node) {
            return;
        }
        let Some(sel) = class_send.message_loc() else { return };
        let (start, end) = (sel.start_offset(), call.location().end_offset());

        let is_str = class_node.as_string_node().is_some();
        let class_name: Option<String> = if !name_method {
            (!is_str).then(|| node_text(class_node).to_string())
        } else if let Some(c) = class_node
            .as_call_node()
            .filter(|c| c.receiver().is_some() && CLASS_NAME_METHODS.contains(&method_name(c)))
        {
            Some(node_text(&c.receiver().unwrap()).to_string())
        } else if is_str {
            let mut value = node_text(class_node).replace(['"', '\''], "");
            let in_class = cx.ancestors.iter().any(|a| matches!(kind::of(a), kind::CLASS_NODE | kind::MODULE_NODE));
            if in_class && !value.starts_with("::") {
                value.insert_str(0, "::");
            }
            Some(value)
        } else if matches!(
            kind::of(class_node),
            kind::LOCAL_VARIABLE_READ_NODE
                | kind::IT_LOCAL_VARIABLE_READ_NODE
                | kind::INSTANCE_VARIABLE_READ_NODE
                | kind::CLASS_VARIABLE_READ_NODE
                | kind::GLOBAL_VARIABLE_READ_NODE
        ) || class_node.as_call_node().is_some_and(|c| c.block().is_none_or(|b| b.as_block_node().is_none()))
        {
            None
        } else {
            Some(node_text(class_node).to_string())
        };
        let class_argument = class_name.map(|n| format!("({n})")).unwrap_or_default();
        let msg = format!("Use `instance_of?{class_argument}` instead of comparing classes.");
        let fixable = !class_argument.is_empty();
        let replacement = format!("instance_of?{class_argument}");
        cx.add_fix(start, end, &msg, |c| {
            if fixable {
                c.replace(start, end, &replacement);
            }
        });
    }
}
