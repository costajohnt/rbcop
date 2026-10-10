use crate::prelude::*;

pub const NAME: &str = "Lint/StructNewOverride";
pub static COP: StructNewOverride = StructNewOverride;
pub struct StructNewOverride;

const STRUCT_METHOD_NAMES: &[&str] = &[
    "!", "!=", "!~", "<=>", "==", "===", "[]", "[]=", "__id__", "__send__", "all?", "any?", "chain", "chunk",
    "chunk_while", "class", "clone", "collect", "collect_concat", "compact", "count", "cycle", "deconstruct",
    "deconstruct_keys", "define_singleton_method", "detect", "dig", "display", "drop", "drop_while", "dup", "each",
    "each_cons", "each_entry", "each_pair", "each_slice", "each_with_index", "each_with_object", "entries",
    "enum_for", "eql?", "equal?", "extend", "filter", "filter_map", "find", "find_all", "find_index", "first",
    "flat_map", "freeze", "frozen?", "grep", "grep_v", "group_by", "hash", "include?", "inject", "inspect",
    "instance_eval", "instance_exec", "instance_of?", "instance_variable_defined?", "instance_variable_get",
    "instance_variable_set", "instance_variables", "is_a?", "itself", "kind_of?", "lazy", "length", "map", "max",
    "max_by", "member?", "members", "method", "methods", "min", "min_by", "minmax", "minmax_by", "nil?", "none?",
    "object_id", "one?", "partition", "private_methods", "protected_methods", "public_method", "public_methods",
    "public_send", "reduce", "reject", "remove_instance_variable", "respond_to?", "reverse_each", "select", "send",
    "singleton_class", "singleton_method", "singleton_methods", "size", "slice_after", "slice_before", "slice_when",
    "sort", "sort_by", "sum", "take", "take_while", "tally", "tap", "then", "to_a", "to_enum", "to_h", "to_s",
    "to_set", "uniq", "values", "values_at", "yield_self", "zip",
];

impl Cop for StructNewOverride {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "new" {
            return;
        }
        let struct_const = call.receiver().is_some_and(|r| match kind::of(&r) {
            kind::CONSTANT_READ_NODE => r.as_constant_read_node().unwrap().name().as_slice() == b"Struct",
            kind::CONSTANT_PATH_NODE => {
                let p = r.as_constant_path_node().unwrap();
                p.parent().is_none() && p.name().is_some_and(|n| n.as_slice() == b"Struct")
            }
            _ => false,
        });
        if !struct_const {
            return;
        }
        for (i, arg) in call_args(&call).iter().enumerate() {
            let (name, inspect) = if let Some(s) = arg.as_symbol_node() {
                let v = String::from_utf8_lossy(s.unescaped()).into_owned();
                (v.clone(), format!(":{v}"))
            } else if let Some(s) = arg.as_string_node() {
                if i == 0 {
                    continue;
                }
                let v = String::from_utf8_lossy(s.unescaped()).into_owned();
                (v.clone(), format!("\"{v}\""))
            } else {
                continue;
            };
            if !STRUCT_METHOD_NAMES.contains(&name.as_str()) {
                continue;
            }
            let l = arg.location();
            let msg = format!("`{inspect}` member overrides `Struct#{name}` and it may be unexpected.");
            cx.add(l.start_offset(), l.end_offset(), msg);
        }
    }
}
