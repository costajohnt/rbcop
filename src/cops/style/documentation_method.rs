use crate::prelude::*;
use helpers::documentation_comment::{comments_taken_by_ancestor, documentation_comment, enclosing_start, node_ends};
use helpers::parser_parent::{parser_parent, parser_parent_index};
use helpers::visibility_help::{node_visibility, non_public_modifier, siblings};

pub const NAME: &str = "Style/DocumentationMethod";
pub static COP: DocumentationMethod = DocumentationMethod;
pub struct DocumentationMethod;

#[derive(Default)]
struct State {
    ends: Option<Vec<usize>>,
}

impl Cop for DocumentationMethod {
    fn kinds(&self) -> &'static [usize] {
        &[kind::DEF_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let def = node.as_def_node().unwrap();
        let name = std::str::from_utf8(def.name().as_slice()).unwrap_or("");
        if name == "initialize" || cx.cfg.strings("AllowedMethods").iter().any(|m| m == name) {
            return;
        }
        let anc = cx.ancestors;
        // `(send nil? {:module_function :ruby2_keywords} ...)` parent: check the send instead.
        let parent_idx = parser_parent_index(anc, anc.len());
        let modifier = parent_idx.filter(|&i| {
            anc[i].as_call_node().is_some_and(|c| {
                c.receiver().is_none()
                    && !is_csend(&c)
                    && matches!(method_name(&c), "module_function" | "ruby2_keywords")
                    && c.block().is_none_or(|b| b.as_block_node().is_none())
            })
        });
        let (target, target_anc): (&Node, &[Node]) = match modifier {
            Some(i) => (&anc[i], &anc[..i]),
            None => (node, anc),
        };
        // non_public?
        let parent = parser_parent(target_anc);
        let (sibs, idx) = siblings(target_anc, target);
        let non_public = non_public_modifier(parent) || node_visibility(target, parent, &sibs, idx) != "public";
        if non_public && !cx.cfg.bool("RequireForNonPublicMethods").unwrap_or(false) {
            return;
        }
        if cx.state::<State>().ends.is_none() {
            let ends = anc.first().map(node_ends).unwrap_or_default();
            cx.state::<State>().ends = Some(ends);
        }
        let ends = cx.state::<State>().ends.take().unwrap();
        let loc = target.location();
        let r = match target.as_call_node() {
            Some(c) => send_range(&c),
            None => (loc.start_offset(), loc.end_offset()),
        };
        let enclosing = enclosing_start(target_anc, r.0);
        let documented =
            !comments_taken_by_ancestor(target_anc, r.0) && documentation_comment(cx, &ends, r.0, enclosing);
        cx.state::<State>().ends = Some(ends);
        if !documented {
            cx.add(r.0, r.1, "Missing method documentation comment.");
        }
    }
}
