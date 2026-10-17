use crate::prelude::*;

pub const NAME: &str = "RSpec/VerifiedDoubleReference";
pub static COP: VerifiedDoubleReference = VerifiedDoubleReference;
pub struct VerifiedDoubleReference;

static CONSTANT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A(?:::)?[A-Z][A-Za-z0-9_]*(?:::[A-Z][A-Za-z0-9_]*)*\z").unwrap());

impl Cop for VerifiedDoubleReference {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? RESTRICT_ON_SEND $str ...)
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some()
            || !matches!(
                method_name(&call),
                "class_double" | "class_spy" | "instance_double" | "instance_spy" | "mock_model" | "object_double" | "object_spy" | "stub_model"
            )
        {
            return;
        }
        let Some(arg) = send_args(&call).into_iter().next() else { return };
        if !helpers::rspec::is_str(&arg) {
            return;
        }
        let value = String::from_utf8_lossy(arg.as_string_node().unwrap().unescaped()).into_owned();
        let (s, e) = helpers::rspec::node_range(&arg);
        let msg = "Use a constant class reference for verified doubles. String references are not verifying unless the class is loaded.";
        // The string's value is written as code: only a constant name is.
        if !CONSTANT.is_match(&value) {
            cx.add_withheld_fix(s, e, msg);
            return;
        }
        cx.add_fix(s, e, msg, |c| c.replace(s, e, &value));
    }
}
