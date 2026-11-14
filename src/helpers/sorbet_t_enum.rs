//! rubocop-sorbet's TEnum mixin: the innermost class being visited and whether it is a `T::Enum`.

use crate::prelude::*;
use ruby_prism::ClassNode;

/// t_enum?: `(class (const ...) (const (const nil? :T) :Enum) ...)`.
pub fn is_t_enum(class: &ClassNode) -> bool {
    class.superclass().and_then(|s| s.as_constant_path_node()).is_some_and(|p| {
        p.name().is_some_and(|n| n.as_slice() == b"Enum")
            && p.parent().and_then(|t| t.as_constant_read_node()).is_some_and(|t| t.name().as_slice() == b"T")
    })
}

/// `@scopes.last`: the innermost class among the ancestors (modules and `class << self` are not
/// pushed).
pub fn innermost_class<'pr>(ancestors: &[Node<'pr>]) -> Option<ClassNode<'pr>> {
    ancestors.iter().rev().find_map(|n| n.as_class_node())
}

/// in_t_enum_class?
pub fn in_t_enum_class(ancestors: &[Node]) -> bool {
    innermost_class(ancestors).is_some_and(|c| is_t_enum(&c))
}
