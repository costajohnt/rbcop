//! RuboCop's GemDeclaration mixin plus the version-specifier / commit-reference matchers shared by
//! Bundler/GemVersion and Gemspec/DependencyVersion.

use crate::prelude::*;
use regex::bytes::Regex as BytesRegex;
use ruby_prism::CallNode;

static VERSION_SPECIFICATION: LazyLock<BytesRegex> =
    LazyLock::new(|| BytesRegex::new(r"(?m-u)^\s*[~<>=]*\s*[0-9.]+").unwrap());

/// Parser `str` (not a multi-line `dstr`): its value.
pub fn str_value(node: &Node) -> Option<Vec<u8>> {
    let s = node.as_string_node()?;
    (!is_parser_dstr(node)).then(|| s.unescaped().to_vec())
}

/// `gem_declaration?`: `(send nil? :gem str ...)`.
pub fn is_gem_declaration(call: &CallNode) -> bool {
    call.receiver().is_none()
        && call.name().as_slice() == b"gem"
        && send_args(call).first().and_then(str_value).is_some()
}

/// `<(str #version_specification?) ...>` over the send's arguments.
pub fn includes_version_specification(call: &CallNode) -> bool {
    send_args(call).iter().any(|a| str_value(a).is_some_and(|v| VERSION_SPECIFICATION.is_match(&v)))
}

/// `<(hash <(pair (sym {:branch :ref :tag}) (str _)) ...>) ...>` over the send's arguments.
pub fn includes_commit_reference(call: &CallNode) -> bool {
    send_args(call).iter().any(|a| {
        let elements = match (a.as_hash_node(), a.as_keyword_hash_node()) {
            (Some(h), _) => h.elements(),
            (_, Some(h)) => h.elements(),
            _ => return false,
        };
        elements.iter().any(|e| {
            e.as_assoc_node().is_some_and(|p| {
                p.key().as_symbol_node().is_some_and(|k| matches!(k.unescaped(), b"branch" | b"ref" | b"tag"))
                    && str_value(&p.value()).is_some()
            })
        })
    })
}
