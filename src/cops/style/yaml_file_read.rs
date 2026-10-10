use crate::helpers::parser_parent::is_top_const;
use crate::prelude::*;

pub const NAME: &str = "Style/YAMLFileRead";
pub static COP: YAMLFileRead = YAMLFileRead;
pub struct YAMLFileRead;

impl Cop for YAMLFileRead {
    fn correctable(&self) -> bool {
        true
    }
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "load" | "safe_load" | "parse") || is_csend(&call) {
            return;
        }
        if name == "safe_load" && cx.config.target_ruby <= 2.7 {
            return;
        }
        // `(send (const {cbase nil?} :YAML) _ (send (const {cbase nil?} :File) :read $_) $...)`
        if !call.receiver().is_some_and(|r| is_top_const(&r, "YAML")) {
            return;
        }
        let args = send_args(&call);
        let Some((first, rest)) = args.split_first() else { return };
        let Some(read) = first.as_call_node() else { return };
        if is_csend(&read) || method_name(&read) != "read" || !read.receiver().is_some_and(|r| is_top_const(&r, "File")) {
            return;
        }
        let read_args = send_args(&read);
        let [file_path] = read_args.as_slice() else { return };
        let rest: String = rest.iter().map(|a| format!(", {}", node_text(a))).collect();
        let prefer = format!("{name}_file({}{rest})", node_text(file_path));
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), send_range(&call).1);
        cx.add_fix(s, e, format!("Use `{prefer}` instead."), |c| c.replace(s, e, &prefer));
    }
}
