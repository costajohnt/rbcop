//! RuboCop's NilMethods mixin: `nil.methods` (as loaded in a RuboCop 1.91 / Ruby 4.0 process),
//! `to_d`, and the cop's AllowedMethods.

use crate::config::CopConfig;

const NIL_METHODS: &[&str] = &[
    "!", "!=", "!~", "&", "<=>", "==", "===", "=~", "^", "__id__", "__send__", "class", "clone",
    "define_singleton_method", "display", "dup", "enum_for", "eql?", "equal?", "extend", "freeze", "frozen?", "hash",
    "inspect", "instance_eval", "instance_exec", "instance_of?", "instance_variable_defined?",
    "instance_variable_get", "instance_variable_set", "instance_variables", "is_a?", "itself", "kind_of?", "method",
    "methods", "nil?", "object_id", "private_methods", "protected_methods", "public_method", "public_methods",
    "public_send", "rationalize", "remove_instance_variable", "respond_to?", "send", "singleton_class",
    "singleton_method", "singleton_methods", "tap", "then", "to_a", "to_c", "to_enum", "to_f", "to_h", "to_i",
    "to_json", "to_r", "to_s", "to_yaml", "yield_self", "|",
];

/// `nil_methods.include?(name)`.
pub fn nil_method(cfg: &CopConfig, name: &str) -> bool {
    NIL_METHODS.contains(&name) || name == "to_d" || crate::helpers::allowed::allowed_method(cfg, name)
}
