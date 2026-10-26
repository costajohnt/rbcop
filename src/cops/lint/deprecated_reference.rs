use crate::helpers::comment_association::{self, Associator};
use crate::prelude::*;
use crate::project_index::{self, Decl, ProjectIndex};
use ruby_prism::Visit;

pub const NAME: &str = "Lint/DeprecatedReference";
pub static COP: DeprecatedReference = DeprecatedReference;
pub struct DeprecatedReference;

static DETAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@deprecated\s+(.+)").unwrap());

/// Only with `AllCops/UseProjectIndex`: references to methods and constants whose every
/// definition carries a YARD `@deprecated` tag.
impl Cop for DeprecatedReference {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::CALL_NODE,
            kind::CALL_TARGET_NODE,
            kind::CALL_OPERATOR_WRITE_NODE,
            kind::CALL_OR_WRITE_NODE,
            kind::CALL_AND_WRITE_NODE,
            kind::CONSTANT_READ_NODE,
            kind::CONSTANT_PATH_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(index) = project_index::get() else { return };
        let anc = cx.ancestors;
        if let Some(send) = project_index::parser_send(node) {
            let Some(sel) = send.selector else { return };
            let Some(decl) = method_declaration(index, anc, &send) else { return };
            let Some(detail) = deprecation(index, decl) else { return };
            if within_deprecated_definition(cx) {
                return;
            }
            let msg = format!("Method `{}` is deprecated{detail}", send.name);
            cx.add(sel.0, sel.1, msg);
            return;
        }
        // A path written to is a parser `casgn`, not a `const`.
        let start = node.location().start_offset();
        let target = cx.parent().is_some_and(|p| match p {
            Node::ConstantPathWriteNode { .. } => p.as_constant_path_write_node().unwrap().target().location().start_offset() == start,
            Node::ConstantPathOrWriteNode { .. } => p.as_constant_path_or_write_node().unwrap().target().location().start_offset() == start,
            Node::ConstantPathAndWriteNode { .. } => p.as_constant_path_and_write_node().unwrap().target().location().start_offset() == start,
            Node::ConstantPathOperatorWriteNode { .. } => {
                p.as_constant_path_operator_write_node().unwrap().target().location().start_offset() == start
            }
            _ => false,
        });
        if target || helpers::parser_parent::defines_module(anc) {
            return;
        }
        let Some(decl) = project_index::resolve_const_node(index, node, anc) else { return };
        let Some(detail) = deprecation(index, decl) else { return };
        if within_deprecated_definition(cx) {
            return;
        }
        let Some(name) = helpers::const_name::const_name(node) else { return };
        let mut end = node.location().end_offset();
        // The omitted value of `{ Foo: }` spans the bare name (see Lint/ConstantResolution).
        if cx.parent().is_some_and(|p| p.as_implicit_node().is_some()) && cx.src.text.as_bytes()[end - 1] == b':' {
            end -= 1;
        }
        cx.add(start, end, format!("Constant `{name}` is deprecated{detail}"));
    }
}

/// `method_declaration_for`.
fn method_declaration(index: &ProjectIndex, anc: &[Node], send: &project_index::Send) -> Option<Decl> {
    let member = format!("{}()", send.name);
    match &send.receiver {
        None => implicit_receiver(index, anc, &member),
        Some(r) if r.as_self_node().is_some() => implicit_receiver(index, anc, &member),
        Some(r) if r.as_constant_read_node().is_some() || r.as_constant_path_node().is_some() => {
            let ns = project_index::resolve_const_node(index, r, anc)?;
            let ix = index.read();
            if !ix.is_namespace(ns) {
                return None;
            }
            ix.singleton_member(ns, &member)
        }
        _ => None,
    }
}

/// `implicit_receiver_declaration`.
fn implicit_receiver(index: &ProjectIndex, anc: &[Node], member: &str) -> Option<Decl> {
    // enclosing_namespace: the nearest class or module, else `Object`.
    let ns = match anc.iter().rposition(|a| a.as_class_node().is_some() || a.as_module_node().is_some()) {
        Some(i) => project_index::resolve_const_node(index, &project_index::identifier(&anc[i])?, &anc[..i])?,
        None => index.read().lookup("Object")?,
    };
    let ix = index.read();
    if !ix.is_namespace(ns) {
        return None;
    }
    if singleton_context(anc) { ix.singleton_member(ns, member) } else { ix.find_member(ns, member) }
}

/// `singleton_context?`.
fn singleton_context(anc: &[Node]) -> bool {
    let mut instance_def_seen = false;
    for a in anc.iter().rev() {
        match a {
            Node::DefNode { .. } if a.as_def_node().unwrap().receiver().is_some() => return true,
            Node::SingletonClassNode { .. } => return true,
            Node::DefNode { .. } => instance_def_seen = true,
            Node::ClassNode { .. } | Node::ModuleNode { .. } => return !instance_def_seen,
            _ => {}
        }
    }
    false
}

/// `deprecated?` and `detail_for`: the message tail when every definition is deprecated.
fn deprecation(index: &ProjectIndex, decl: Decl) -> Option<String> {
    let ix = index.read();
    let defs = ix.definitions(decl);
    if defs.is_empty() || !defs.iter().all(|&d| ix.deprecated(d)) {
        return None;
    }
    let tag = defs.iter().find_map(|&d| ix.comments(d).into_iter().find(|c| c.contains("@deprecated")));
    let text = tag.and_then(|t| DETAIL.captures(&t).map(|c| helpers::ruby_string::strip(&c[1]).to_string()));
    Some(match text {
        None => ".".to_string(),
        Some(t) if t.is_empty() => ".".to_string(),
        Some(t) if t.ends_with('.') => format!(": {t}"),
        Some(t) => format!(": {t}."),
    })
}

/// `within_deprecated_definition?`: a def, class or module around the node has a comment
/// (`ast_with_comments`) mentioning `@deprecated`.
fn within_deprecated_definition(cx: &Cx) -> bool {
    let src = cx.src;
    let scopes: Vec<(usize, usize)> = cx
        .ancestors
        .iter()
        .filter(|a| matches!(a, Node::DefNode { .. } | Node::ClassNode { .. } | Node::ModuleNode { .. }))
        .map(|a| (a.location().start_offset(), a.location().end_offset()))
        .collect();
    if scopes.is_empty() || !src.comments.iter().any(|c| src.slice(c.start, c.end).contains("@deprecated")) {
        return false;
    }
    let mut a = Associator::new(src, comment_association::skipped_directives(src));
    a.visit(&cx.ancestors[0]);
    scopes.iter().any(|&(s, e)| a.comments_of(s, e).any(|c| src.slice(c.start, c.end).contains("@deprecated")))
}
