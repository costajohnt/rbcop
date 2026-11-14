use crate::helpers::parser_parent::{is_top_const, parser_parent_index};
use crate::helpers::sorbet_rbs_assertion::{
    assertion_statement, assigns, autocorrectable, comments_within, no_literal_block, parser_range, rbi_type_to_rbs,
};
use crate::prelude::*;

pub const NAME: &str = "Sorbet/ForbidTCollectionInstantiation";
pub static COP: ForbidTCollectionInstantiation = ForbidTCollectionInstantiation;
pub struct ForbidTCollectionInstantiation;

const MSG: &str = "Instantiate Ruby collections directly instead of Sorbet collection types.";

/// `t_collection?`: `T::Array` .. `T::Enumerator` or `T::Enumerator::Lazy`/`::Chain` (`T` or `::T`).
fn t_collection(node: &Node) -> bool {
    let Some(p) = node.as_constant_path_node() else { return false };
    let Some(parent) = p.parent() else { return false };
    match p.name().map(|n| n.as_slice()) {
        Some(b"Array" | b"Hash" | b"Set" | b"Range" | b"Enumerator") => is_top_const(&parent, "T"),
        Some(b"Lazy" | b"Chain") => parent.as_constant_path_node().is_some_and(|e| {
            e.name().is_some_and(|n| n.as_slice() == b"Enumerator") && e.parent().is_some_and(|t| is_top_const(&t, "T"))
        }),
        _ => false,
    }
}

enum Edit {
    Replace(usize, usize, String),
    InsertAfter(usize, usize, String),
}

impl Cop for ForbidTCollectionInstantiation {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "new" {
            return;
        }
        let Some(receiver) = call.receiver() else { return };
        let generic = receiver.as_call_node().filter(|r| !is_csend(r) && no_literal_block(r));
        let ty = match &generic {
            Some(g) if method_name(g) == "[]" => g.receiver(),
            _ => call.receiver(),
        };
        let Some(ty) = ty.filter(t_collection) else { return };
        let edits = if cx.cfg.bool("AutocorrectToRBS").unwrap_or(false) { self.edits(cx, &call, &receiver, &ty) } else { None };
        let (s, e) = parser_range(&receiver);
        cx.add_fix(s, e, MSG, |c| {
            for edit in edits.iter().flatten() {
                match edit {
                    Edit::Replace(a, b, t) => c.replace(*a, *b, t),
                    Edit::InsertAfter(a, b, t) => c.insert_after(*a, *b, t),
                }
            }
        });
    }
}

impl ForbidTCollectionInstantiation {
    /// `autocorrect_to_rbs`.
    fn edits(&self, cx: &Cx, call: &ruby_prism::CallNode, receiver: &Node, ty: &Node) -> Option<Vec<Edit>> {
        let src = cx.src;
        let ancestors = cx.ancestors;
        let has_block = !no_literal_block(call);
        let send = send_range(call);
        // expression = node.block_node || node, climbing out of one-child parentheses.
        let mut expression = if has_block { helpers::node_types::range(&call.as_node()) } else { send };
        // (with a block, the block's parser parent: the call's)
        let mut parent = parser_parent_index(ancestors, ancestors.len());
        while let Some(i) = parent
            && let Some(p) = ancestors[i].as_parentheses_node()
            && p.body().and_then(|b| b.as_statements_node()).is_some_and(|s| s.body().iter().count() == 1)
        {
            expression = helpers::node_types::range(&ancestors[i]);
            parent = parser_parent_index(ancestors, i);
        }
        let assignment = parent.map(|i| &ancestors[i]);
        let generic = receiver.as_call_node().filter(|r| !is_csend(r) && no_literal_block(r));
        let annotate = generic.is_some() && assigns(assignment, expression);
        if annotate && !autocorrectable(cx, expression, assertion_statement(assignment, expression, true)) {
            return None;
        }
        let recv_range = parser_range(receiver);
        if comments_within(src, recv_range) {
            return None;
        }
        let name = helpers::const_name::const_name(ty)?;
        let mut ruby_class = name.strip_prefix("T::").unwrap_or(&name).to_string();
        if node_text(ty).starts_with("::") {
            ruby_class = format!("::{ruby_class}");
        }
        let mut edits = Vec::new();
        if annotate {
            let args: Option<Vec<String>> = send_args(generic.as_ref().unwrap()).iter().map(|a| rbi_type_to_rbs(node_text(a))).collect();
            edits.push(Edit::InsertAfter(expression.0, expression.1, format!(" #: {ruby_class}[{}]", args?.join(", "))));
        }
        // empty_collection_literal
        let short = ty.as_constant_path_node().and_then(|p| p.name()).map(|n| n.as_slice());
        let literal = if send_args(call).is_empty() && !has_block && !comments_within(src, send) {
            match short {
                Some(b"Array") => Some("[]"),
                Some(b"Hash") => {
                    let in_unparenthesized_call = crate::helpers::sorbet_rbs_assertion::send_parent(call, ancestors)
                        .and_then(|p| p.as_call_node())
                        .is_some_and(|p| p.opening_loc().is_none() && send_args(&p).iter().any(|a| parser_range(a) == send));
                    Some(if in_unparenthesized_call { "({})" } else { "{}" })
                }
                _ => None,
            }
        } else {
            None
        };
        match literal {
            Some(l) => edits.push(Edit::Replace(send.0, send.1, l.to_string())),
            None => edits.push(Edit::Replace(recv_range.0, recv_range.1, ruby_class)),
        }
        Some(edits)
    }
}
