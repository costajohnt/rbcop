use crate::prelude::*;

pub const NAME: &str = "Performance/StringIdentifierArgument";
pub static COP: StringIdentifierArgument = StringIdentifierArgument;
pub struct StringIdentifierArgument;

#[rustfmt::skip]
const COMMAND_METHODS: &[&str] = &[
    "alias_method", "attr_accessor", "attr_reader", "attr_writer", "autoload", "autoload?", "private",
    "private_constant", "protected", "public", "public_constant", "module_function",
];
#[rustfmt::skip]
const MULTIPLE_ARGUMENTS_METHODS: &[&str] = &[
    "attr_accessor", "attr_reader", "attr_writer", "private", "private_constant", "protected", "public",
    "public_constant", "module_function",
];
#[rustfmt::skip]
const RESTRICT_ON_SEND: &[&str] = &[
    "class_variable_defined?", "const_set", "define_method", "instance_method", "method_defined?",
    "private_class_method?", "private_method_defined?", "protected_method_defined?", "public_class_method",
    "public_instance_method", "public_method_defined?", "remove_class_variable", "remove_method", "undef_method",
    "class_variable_get", "class_variable_set", "deprecate_constant", "remove_const", "ruby2_keywords",
    "define_singleton_method", "instance_variable_defined?", "instance_variable_get", "instance_variable_set",
    "method", "public_method", "public_send", "remove_instance_variable", "respond_to?", "send",
    "singleton_method", "__send__",
];

impl Cop for StringIdentifierArgument {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        let command = COMMAND_METHODS.contains(&name);
        if is_csend(&call) || !(command || RESTRICT_ON_SEND.contains(&name)) {
            return;
        }
        if command && call.receiver().is_some() {
            return;
        }
        let args = send_args(&call);
        let candidates: Vec<&Node> = if name == "alias_method" {
            args.iter().take(2).collect()
        } else if MULTIPLE_ARGUMENTS_METHODS.contains(&name) {
            args.iter().collect()
        } else {
            args.iter().take(1).collect()
        };
        for arg in candidates {
            let Some(s) = arg.as_string_node().filter(|_| !is_parser_dstr(arg)) else { continue };
            let value = s.unescaped();
            if value.contains(&b' ') || value.windows(2).any(|w| w == b"::") {
                continue;
            }
            let replacement = helpers::ruby_string::symbol_inspect(value);
            let loc = arg.location();
            let (start, end) = (loc.start_offset(), loc.end_offset());
            let msg = format!("Use `{replacement}` instead of `{}`.", node_text(arg));
            cx.add_fix(start, end, msg, |c| c.replace(start, end, &replacement));
        }
    }
}
