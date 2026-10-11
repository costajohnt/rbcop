//! ConfigurableNaming / ConfigurableNumbering FORMATS plus the AllowedIdentifiers,
//! ForbiddenIdentifiers and ForbiddenPattern mixins used by the Naming cops.
//! Ruby `[[:lower:]]`/`[[:upper:]]` are the Unicode Lowercase/Uppercase properties; `\d` is ASCII.

use crate::config::CopConfig;
use regex::Regex;
use std::sync::LazyLock;

static NAMING_SNAKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^@{0,2}[0-9\p{Lowercase}_]+[!?=]?$").unwrap());
static NAMING_CAMEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^@{0,2}(?:_|_?[\p{Lowercase}][0-9\p{Lowercase}\p{Uppercase}]*)[!?=]?$").unwrap()
});
static NUM_SNAKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:[^0-9]|_[0-9]+|\A[0-9]+)\z").unwrap());
static NUM_NORMAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:[^0-9]|[^_0-9][0-9]+|\A[0-9]+)\z|\A_[0-9]+\z").unwrap());
static NUM_NON_INTEGER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:[^0-9]|\A[0-9]+)\z|\A_[0-9]+\z").unwrap());

/// ConfigurableNaming::FORMATS (`snake_case`, `camelCase`).
pub fn naming_valid(style: &str, name: &str) -> bool {
    match style {
        "camelCase" => NAMING_CAMEL.is_match(name),
        _ => NAMING_SNAKE.is_match(name),
    }
}

/// ConfigurableNumbering::FORMATS (`snake_case`, `normalcase`, `non_integer`).
pub fn numbering_valid(style: &str, name: &str) -> bool {
    match style {
        "snake_case" => NUM_SNAKE.is_match(name),
        "non_integer" => NUM_NON_INTEGER.is_match(name),
        _ => NUM_NORMAL.is_match(name),
    }
}

fn strip_sigils(name: &str) -> String {
    name.chars().filter(|&c| c != '@' && c != '$').collect()
}

/// AllowedIdentifiers#allowed_identifier? (sigils `@`/`$` removed first).
pub fn allowed_identifier(cfg: &CopConfig, name: &str) -> bool {
    let name = strip_sigils(name);
    cfg.strings("AllowedIdentifiers").contains(&name)
}

/// ForbiddenIdentifiers#forbidden_identifier? || ForbiddenPattern#forbidden_pattern?
pub fn forbidden_name(cfg: &CopConfig, name: &str) -> bool {
    let bare = strip_sigils(name);
    cfg.strings("ForbiddenIdentifiers").contains(&bare)
        || cfg.regexes("ForbiddenPatterns").iter().any(|r| r.is_match(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert!(naming_valid("snake_case", "@foo_bar?"));
        assert!(!naming_valid("snake_case", "fooBar"));
        assert!(naming_valid("camelCase", "fooBar"));
        assert!(!naming_valid("camelCase", "foo_bar"));
        assert!(numbering_valid("normalcase", "foo1"));
        assert!(!numbering_valid("normalcase", "foo_1"));
        assert!(numbering_valid("normalcase", "_1"));
        assert!(numbering_valid("snake_case", "foo_1"));
        assert!(!numbering_valid("snake_case", "foo1"));
        assert!(!numbering_valid("non_integer", "foo1"));
    }
}

/// What a parser variable/argument node is (VariableName / VariableNumber dispatch).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    /// `arg` (a required positional parameter)
    Arg,
    /// `optarg`, `restarg`, `kwarg`, `kwoptarg`, `kwrestarg`, `blockarg`
    OtherArg,
    Lvasgn,
    Ivasgn,
    Cvasgn,
    Gvasgn,
    Lvar,
}

/// `(kind, node.name, loc.name)` for the parser nodes the variable-naming cops handle; `None` for
/// other nodes, nameless parameters (`*`, `**`, `&`) and pattern / regexp-capture targets.
pub fn variable(node: &ruby_prism::Node, ancestors: &[ruby_prism::Node]) -> Option<(VarKind, String, usize, usize)> {
    use ruby_prism::Node as N;
    use VarKind::*;
    let s = |id: ruby_prism::ConstantId| String::from_utf8_lossy(id.as_slice()).into_owned();
    let l = |loc: ruby_prism::Location| (loc.start_offset(), loc.end_offset());
    let (k, name, (start, end)) = match node {
        N::RequiredParameterNode { .. } => {
            let n = node.as_required_parameter_node()?;
            (Arg, s(n.name()), l(n.location()))
        }
        N::OptionalParameterNode { .. } => {
            let n = node.as_optional_parameter_node()?;
            (OtherArg, s(n.name()), l(n.name_loc()))
        }
        N::RestParameterNode { .. } => {
            let n = node.as_rest_parameter_node()?;
            (OtherArg, s(n.name()?), l(n.name_loc()?))
        }
        N::KeywordRestParameterNode { .. } => {
            let n = node.as_keyword_rest_parameter_node()?;
            (OtherArg, s(n.name()?), l(n.name_loc()?))
        }
        N::BlockParameterNode { .. } => {
            let n = node.as_block_parameter_node()?;
            (OtherArg, s(n.name()?), l(n.name_loc()?))
        }
        // parser's kwarg/kwoptarg name excludes the label colon
        N::RequiredKeywordParameterNode { .. } => {
            let n = node.as_required_keyword_parameter_node()?;
            let st = n.name_loc().start_offset();
            (OtherArg, s(n.name()), (st, st + n.name().as_slice().len()))
        }
        N::OptionalKeywordParameterNode { .. } => {
            let n = node.as_optional_keyword_parameter_node()?;
            let st = n.name_loc().start_offset();
            (OtherArg, s(n.name()), (st, st + n.name().as_slice().len()))
        }
        // parser's lvar is the bare name (Prism's `foo(name:)` read spans the colon)
        N::LocalVariableReadNode { .. } => {
            let n = node.as_local_variable_read_node()?;
            (Lvar, s(n.name()), crate::helpers::lvar::range(&n.location(), n.name().as_slice()))
        }
        // the implicit `it` block parameter (3.4+) is an lvar for parser
        N::ItLocalVariableReadNode { .. } => (Lvar, "it".to_string(), crate::helpers::lvar::range(&node.location(), b"it")),
        N::LocalVariableWriteNode { .. } => {
            let n = node.as_local_variable_write_node()?;
            (Lvasgn, s(n.name()), l(n.name_loc()))
        }
        N::LocalVariableOperatorWriteNode { .. } => {
            let n = node.as_local_variable_operator_write_node()?;
            (Lvasgn, s(n.name()), l(n.name_loc()))
        }
        N::LocalVariableOrWriteNode { .. } => {
            let n = node.as_local_variable_or_write_node()?;
            (Lvasgn, s(n.name()), l(n.name_loc()))
        }
        N::LocalVariableAndWriteNode { .. } => {
            let n = node.as_local_variable_and_write_node()?;
            (Lvasgn, s(n.name()), l(n.name_loc()))
        }
        N::LocalVariableTargetNode { .. } => {
            // lvasgn only in masgn / rescue / for; elsewhere a pattern `match_var` or regexp capture
            let masgn = |p: Option<&N>| {
                matches!(p, Some(N::MultiWriteNode { .. } | N::MultiTargetNode { .. } | N::RescueNode { .. } | N::ForNode { .. }))
            };
            let n = ancestors.len();
            let parent = ancestors.last();
            let ok = masgn(parent)
                || (parent.is_some_and(|p| p.as_splat_node().is_some()) && masgn(n.checked_sub(2).map(|i| &ancestors[i])));
            if !ok {
                return None;
            }
            let n = node.as_local_variable_target_node()?;
            (Lvasgn, s(n.name()), l(n.location()))
        }
        N::InstanceVariableWriteNode { .. } => {
            let n = node.as_instance_variable_write_node()?;
            (Ivasgn, s(n.name()), l(n.name_loc()))
        }
        N::InstanceVariableOperatorWriteNode { .. } => {
            let n = node.as_instance_variable_operator_write_node()?;
            (Ivasgn, s(n.name()), l(n.name_loc()))
        }
        N::InstanceVariableOrWriteNode { .. } => {
            let n = node.as_instance_variable_or_write_node()?;
            (Ivasgn, s(n.name()), l(n.name_loc()))
        }
        N::InstanceVariableAndWriteNode { .. } => {
            let n = node.as_instance_variable_and_write_node()?;
            (Ivasgn, s(n.name()), l(n.name_loc()))
        }
        N::InstanceVariableTargetNode { .. } => {
            let n = node.as_instance_variable_target_node()?;
            (Ivasgn, s(n.name()), l(n.location()))
        }
        N::ClassVariableWriteNode { .. } => {
            let n = node.as_class_variable_write_node()?;
            (Cvasgn, s(n.name()), l(n.name_loc()))
        }
        N::ClassVariableOperatorWriteNode { .. } => {
            let n = node.as_class_variable_operator_write_node()?;
            (Cvasgn, s(n.name()), l(n.name_loc()))
        }
        N::ClassVariableOrWriteNode { .. } => {
            let n = node.as_class_variable_or_write_node()?;
            (Cvasgn, s(n.name()), l(n.name_loc()))
        }
        N::ClassVariableAndWriteNode { .. } => {
            let n = node.as_class_variable_and_write_node()?;
            (Cvasgn, s(n.name()), l(n.name_loc()))
        }
        N::ClassVariableTargetNode { .. } => {
            let n = node.as_class_variable_target_node()?;
            (Cvasgn, s(n.name()), l(n.location()))
        }
        N::GlobalVariableWriteNode { .. } => {
            let n = node.as_global_variable_write_node()?;
            (Gvasgn, s(n.name()), l(n.name_loc()))
        }
        N::GlobalVariableOperatorWriteNode { .. } => {
            let n = node.as_global_variable_operator_write_node()?;
            (Gvasgn, s(n.name()), l(n.name_loc()))
        }
        N::GlobalVariableOrWriteNode { .. } => {
            let n = node.as_global_variable_or_write_node()?;
            (Gvasgn, s(n.name()), l(n.name_loc()))
        }
        N::GlobalVariableAndWriteNode { .. } => {
            let n = node.as_global_variable_and_write_node()?;
            (Gvasgn, s(n.name()), l(n.name_loc()))
        }
        N::GlobalVariableTargetNode { .. } => {
            let n = node.as_global_variable_target_node()?;
            (Gvasgn, s(n.name()), l(n.location()))
        }
        _ => return None,
    };
    Some((k, name, start, end))
}

/// Node kinds `variable` understands.
pub const VARIABLE_KINDS: &[usize] = &[
    crate::node_kind::REQUIRED_PARAMETER_NODE,
    crate::node_kind::OPTIONAL_PARAMETER_NODE,
    crate::node_kind::REST_PARAMETER_NODE,
    crate::node_kind::KEYWORD_REST_PARAMETER_NODE,
    crate::node_kind::BLOCK_PARAMETER_NODE,
    crate::node_kind::REQUIRED_KEYWORD_PARAMETER_NODE,
    crate::node_kind::OPTIONAL_KEYWORD_PARAMETER_NODE,
    crate::node_kind::LOCAL_VARIABLE_READ_NODE,
    crate::node_kind::IT_LOCAL_VARIABLE_READ_NODE,
    crate::node_kind::LOCAL_VARIABLE_WRITE_NODE,
    crate::node_kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
    crate::node_kind::LOCAL_VARIABLE_OR_WRITE_NODE,
    crate::node_kind::LOCAL_VARIABLE_AND_WRITE_NODE,
    crate::node_kind::LOCAL_VARIABLE_TARGET_NODE,
    crate::node_kind::INSTANCE_VARIABLE_WRITE_NODE,
    crate::node_kind::INSTANCE_VARIABLE_OPERATOR_WRITE_NODE,
    crate::node_kind::INSTANCE_VARIABLE_OR_WRITE_NODE,
    crate::node_kind::INSTANCE_VARIABLE_AND_WRITE_NODE,
    crate::node_kind::INSTANCE_VARIABLE_TARGET_NODE,
    crate::node_kind::CLASS_VARIABLE_WRITE_NODE,
    crate::node_kind::CLASS_VARIABLE_OPERATOR_WRITE_NODE,
    crate::node_kind::CLASS_VARIABLE_OR_WRITE_NODE,
    crate::node_kind::CLASS_VARIABLE_AND_WRITE_NODE,
    crate::node_kind::CLASS_VARIABLE_TARGET_NODE,
    crate::node_kind::GLOBAL_VARIABLE_WRITE_NODE,
    crate::node_kind::GLOBAL_VARIABLE_OPERATOR_WRITE_NODE,
    crate::node_kind::GLOBAL_VARIABLE_OR_WRITE_NODE,
    crate::node_kind::GLOBAL_VARIABLE_AND_WRITE_NODE,
    crate::node_kind::GLOBAL_VARIABLE_TARGET_NODE,
];

/// ConfigurableFormatting#class_emitter_method? for a `defs` (given its Prism ancestors): walk
/// up through enclosing `defs`, then a sibling `class <name>` in the parser parent makes it valid.
pub fn class_emitter_method(ancestors: &[ruby_prism::Node], name: &str) -> bool {
    use ruby_prism::Node as N;
    // index of the parser parent of the node whose Prism parent is ancestors[i]
    let parser_parent = |i: usize| match &ancestors[i] {
        N::StatementsNode { .. } if ancestors[i].as_statements_node().unwrap().body().len() == 1 => i.checked_sub(1),
        _ => Some(i),
    };
    let Some(mut cur) = ancestors.len().checked_sub(1).and_then(parser_parent) else { return false };
    while ancestors[cur].as_def_node().is_some_and(|d| d.receiver().is_some()) {
        let Some(next) = cur.checked_sub(1).and_then(parser_parent) else { return false };
        cur = next;
    }
    let Some(stmts) = ancestors[cur].as_statements_node() else { return false };
    stmts.body().iter().any(|c| c.as_class_node().is_some_and(|k| crate::ast::node_text(&k.constant_path()) == name))
}
