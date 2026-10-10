use crate::prelude::*;

pub const NAME: &str = "Lint/DataDefineOverride";
pub static COP: DataDefineOverride = DataDefineOverride;
pub struct DataDefineOverride;

const DATA_METHOD_NAMES: &[&str] = &[
    "!", "!=", "!~", "<=>", "==", "===", "__id__", "__send__", "class", "clone", "deconstruct", "deconstruct_keys",
    "define_singleton_method", "display", "dup", "enum_for", "eql?", "equal?", "extend", "freeze", "frozen?", "hash",
    "inspect", "instance_eval", "instance_exec", "instance_of?", "instance_variable_defined?", "instance_variable_get",
    "instance_variable_set", "instance_variables", "is_a?", "itself", "kind_of?", "members", "method", "methods",
    "nil?", "object_id", "private_methods", "protected_methods", "public_method", "public_methods", "public_send",
    "remove_instance_variable", "respond_to?", "send", "singleton_class", "singleton_method", "singleton_methods",
    "tap", "then", "to_enum", "to_h", "to_s", "with", "yield_self",
];

impl Cop for DataDefineOverride {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "define" {
            return;
        }
        if !call.receiver().is_some_and(|r| helpers::parser_parent::is_top_const(&r, "Data")) {
            return;
        }
        for arg in call_args(&call) {
            let (name, inspect) = if let Some(s) = arg.as_symbol_node() {
                let v = String::from_utf8_lossy(s.unescaped()).into_owned();
                (v.clone(), format!(":{v}"))
            } else if arg.as_string_node().is_some() && !is_parser_dstr(&arg) {
                let v = String::from_utf8_lossy(arg.as_string_node().unwrap().unescaped()).into_owned();
                (v.clone(), format!("\"{v}\""))
            } else {
                continue;
            };
            if !DATA_METHOD_NAMES.contains(&name.as_str()) {
                continue;
            }
            let l = arg.location();
            let msg = format!("`{inspect}` member overrides `Data#{name}` and it may be unexpected.");
            cx.add(l.start_offset(), l.end_offset(), msg);
        }
    }
}
