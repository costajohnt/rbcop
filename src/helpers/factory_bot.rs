//! rubocop-factory_bot's shared pieces: RuboCop::FactoryBot::Language, the method lists of
//! RuboCop::FactoryBot and ConfigurableExplicitOnly.

use crate::prelude::*;

/// Language::METHODS
pub const METHODS: &[&str] = &[
    "attributes_for",
    "attributes_for_list",
    "attributes_for_pair",
    "build",
    "build_list",
    "build_pair",
    "build_stubbed",
    "build_stubbed_list",
    "build_stubbed_pair",
    "create",
    "create_list",
    "create_pair",
    "generate",
    "generate_list",
    "null",
    "null_list",
    "null_pair",
];

/// ATTRIBUTE_DEFINING_METHODS
pub const ATTRIBUTE_DEFINING_METHODS: &[&str] = &["factory", "ignore", "trait", "traits_for_enum", "transient"];

/// RESERVED_METHODS: DEFINITION_PROXY_METHODS + UNPROXIED_METHODS + ATTRIBUTE_DEFINING_METHODS
pub const RESERVED_METHODS: &[&str] = &[
    "add_attribute",
    "after",
    "association",
    "before",
    "callback",
    "ignore",
    "initialize_with",
    "sequence",
    "skip_create",
    "to_create",
    "__send__",
    "__id__",
    "nil?",
    "send",
    "object_id",
    "extend",
    "instance_eval",
    "initialize",
    "block_given?",
    "raise",
    "caller",
    "method",
    "factory",
    "ignore",
    "trait",
    "traits_for_enum",
    "transient",
];

/// factory_bot?: `(const {nil? cbase} {:FactoryGirl :FactoryBot})`
pub fn factory_bot_const(node: &Node) -> bool {
    helpers::parser_parent::is_top_const(node, "FactoryBot") || helpers::parser_parent::is_top_const(node, "FactoryGirl")
}

/// ConfigurableExplicitOnly#factory_call?: the receiver of a factory call.
pub fn factory_call(cx: &Cx, receiver: Option<&Node>) -> bool {
    match receiver {
        Some(r) => factory_bot_const(r),
        None => cx.cfg.bool("ExplicitOnly") != Some(true),
    }
}
