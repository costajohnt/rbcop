//! Lint/Syntax as RuboCop reports it through Prism::Translation::Parser (prism 1.9):
//! some Prism error types are re-worded with the parser gem's message templates.

/// Prism errors the parser gem reports from semantic actions or its lexer (its messages are the
/// templates below) rather than as a grammar failure (`unexpected token ...`).
pub fn is_semantic(kind: &str) -> bool {
    kind.starts_with("argument_formal_")
        || kind.starts_with("argument_no_forwarding")
        || kind.starts_with("incomplete_variable_")
        || kind.starts_with("invalid_variable_global")
        || kind.starts_with("numbered_parameter_")
        || matches!(
            kind,
            "argument_block_multi"
                | "begin_lonely_else"
                | "class_name"
                | "module_name"
                | "class_in_method"
                | "module_in_method"
                | "def_endless_setter"
                | "embdoc_term"
                | "parameter_circular"
                | "parameter_name_repeat"
                | "parameter_numbered_reserved"
                | "regexp_unknown_options"
                | "singleton_for_literals"
                | "string_literal_eof"
                | "write_target_in_method"
        )
}

/// (start, end, message) of a Prism error after the translation layer's rewrites.
pub fn translate(kind: &str, prism_message: &str, start: usize, end: usize, text: &str) -> (usize, usize, String) {
    let slice = |s: usize, e: usize| text.get(s..e.min(text.len())).unwrap_or("").to_string();
    let fixed = |m: &str| m.to_string();
    let (start, end, msg) = match kind {
        "argument_block_multi" => (start, end, fixed("both block argument and literal block are passed")),
        "argument_formal_constant" => (start, end, fixed("formal argument cannot be a constant")),
        "argument_formal_class" => (start, end, fixed("formal argument cannot be a class variable")),
        "argument_formal_global" => (start, end, fixed("formal argument cannot be a global variable")),
        "argument_formal_ivar" => (start, end, fixed("formal argument cannot be an instance variable")),
        "argument_no_forwarding_amp" | "argument_no_forwarding_ampersand" => {
            (start, end, fixed("no anonymous block parameter"))
        }
        "argument_no_forwarding_star" => (start, end, fixed("no anonymous rest parameter")),
        "argument_no_forwarding_star_star" => (start, end, fixed("no anonymous keyword rest parameter")),
        "begin_lonely_else" => (start, start + 4, fixed("else without rescue is useless")),
        "class_name" | "module_name" => (start, end, fixed("class or module name must be a constant literal")),
        "class_in_method" => (start, end, fixed("class definition in method body")),
        "def_endless_setter" => (start, end, fixed("setter method cannot be defined in an endless method definition")),
        "embdoc_term" => {
            (start, end, fixed("embedded document meets end of file (and they embark on a romantic journey)"))
        }
        "incomplete_variable_class" | "incomplete_variable_class_3_3" => {
            let e = end + 1;
            (start, e, format!("`{}' is not allowed as a class variable name", slice(start, e)))
        }
        "incomplete_variable_instance" | "incomplete_variable_instance_3_3" => {
            let e = end + 1;
            (start, e, format!("`{}' is not allowed as an instance variable name", slice(start, e)))
        }
        "invalid_variable_global" | "invalid_variable_global_3_3" => {
            (start, end, format!("`{}' is not allowed as a global variable name", slice(start, end)))
        }
        "module_in_method" => (start, end, fixed("module definition in method body")),
        "numbered_parameter_ordinary" => (start, end, fixed("ordinary parameter is defined")),
        "numbered_parameter_outer_scope" => (start, end, fixed("numbered parameter is already used in an outer scope")),
        "parameter_circular" => (start, end, format!("circular argument reference {}", slice(start, end))),
        "parameter_name_repeat" => (start, end, fixed("duplicate argument name")),
        "parameter_numbered_reserved" => {
            (start, end, format!("{} is reserved for numbered parameter", slice(start, end)))
        }
        "regexp_unknown_options" => {
            let s = slice(start, end);
            (start, end, format!("unknown regexp options: {}", s.get(1..).unwrap_or("")))
        }
        "singleton_for_literals" => (start, end, fixed("cannot define a singleton method for a literal")),
        "string_literal_eof" => (start, end, fixed("unterminated string meets end of file")),
        "unexpected_token_ignore" => (start, end, format!("unexpected token {}", slice(start, end))),
        "write_target_in_method" => (start, end, fixed("dynamic constant assignment")),
        _ => (start, end, prism_message.to_string()),
    };
    // Lint/Syntax#diagnostic_location: zero-width locations grow one character to the right,
    // or (at end of file) one to the left.
    if start == end {
        if end < text.len() {
            let len = text[start..].chars().next().map_or(1, char::len_utf8);
            return (start, start + len, msg);
        }
        if start > 0 {
            let len = text[..start].chars().next_back().map_or(1, char::len_utf8);
            return (start - len, end, msg);
        }
    }
    (start, end, msg)
}
