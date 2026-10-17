use crate::prelude::*;

pub const NAME: &str = "RSpecRails/FileFixture";
pub static COP: FileFixture = FileFixture;
pub struct FileFixture;

fn str_is(node: &Node, value: &str) -> bool {
    helpers::rspec::is_str(node) && node.as_string_node().unwrap().unescaped() == value.as_bytes()
}

impl Cop for FileFixture {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send (send (const {nil? cbase} :Rails) :root) :join {(str "spec") (str "fixtures") | (str "spec/fixtures")} _ ...)
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "join" {
            return;
        }
        let Some(root) = call.receiver().and_then(|r| helpers::rspec::send_call(&r)) else { return };
        if is_csend(&root)
            || method_name(&root) != "root"
            || !send_args(&root).is_empty()
            || !root.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Rails"))
        {
            return;
        }
        let args = send_args(&call);
        let ok = (args.len() >= 3 && str_is(&args[0], "spec") && str_is(&args[1], "fixtures"))
            || (args.len() >= 2 && str_is(&args[0], "spec/fixtures"));
        if ok {
            let sel = call.message_loc().unwrap();
            cx.add(sel.start_offset(), sel.end_offset(), "Prefer `file_fixture` for files under `spec/fixtures`.");
        }
    }
}
