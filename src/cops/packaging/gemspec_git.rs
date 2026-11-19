use crate::helpers::parser_parent::is_top_const;
use crate::helpers::thread_safety::each_node;
use crate::prelude::*;

pub const NAME: &str = "Packaging/GemspecGit";
pub static COP: GemspecGit = GemspecGit;
pub struct GemspecGit;

impl Cop for GemspecGit {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn restrict_on_send(&self) -> &'static [&'static str] {
        &["new"]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (block (send (const (const {cbase nil?} :Gem) :Specification) :new) (args (arg _)) `$(xstr (str #starts_with_git?)))
        let call = node.as_call_node().unwrap();
        let Some(block) = call.block().and_then(|b| b.as_block_node()) else { return };
        let spec = call.receiver().and_then(|r| r.as_constant_path_node()).is_some_and(|p| {
            p.name().is_some_and(|n| n.as_slice() == b"Specification") && p.parent().is_some_and(|g| is_top_const(&g, "Gem"))
        });
        if !spec || is_csend(&call) || method_name(&call) != "new" || !send_args(&call).is_empty() {
            return;
        }
        if !one_arg(block.parameters()) {
            return;
        }
        let Some(body) = block.body() else { return };
        // The descend captures the first match only: one offense per block.
        if let Some(x) = each_node(&body).into_iter().find(git_xstr) {
            let loc = x.location();
            cx.add(
                loc.start_offset(),
                loc.end_offset(),
                "Avoid using git to produce lists of files. Downstreams often need to build your package in an \
                 environment that does not have git (on purpose). Use some pure Ruby alternative, like `Dir` or \
                 `Dir.glob`.",
            );
        }
    }
}

/// `(args (arg _))`: one required parameter (`|s|`, `|s,|`), nothing else.
fn one_arg(params: Option<Node>) -> bool {
    let Some(bp) = params.and_then(|p| p.as_block_parameters_node()) else { return false };
    let Some(p) = bp.parameters() else { return false };
    bp.locals().iter().next().is_none()
        && p.requireds().iter().count() == 1
        && p.requireds().iter().all(|r| r.as_required_parameter_node().is_some())
        && p.optionals().iter().next().is_none()
        && p.rest().is_none_or(|r| r.as_implicit_rest_node().is_some())
        && p.posts().iter().next().is_none()
        && p.keywords().iter().next().is_none()
        && p.keyword_rest().is_none()
        && p.block().is_none()
}

/// `(xstr (str #starts_with_git?))`: a command literal of one line (parser splits a multi-line one
/// into a `str` per line) whose value starts with "git".
// ponytail: a backslash-newline inside the backticks is not treated as a continuation.
fn git_xstr(node: &Node) -> bool {
    let Some(x) = node.as_x_string_node() else { return false };
    let content = x.content_loc().as_slice();
    let body = content.strip_suffix(b"\n").unwrap_or(content);
    !content.is_empty() && !body.contains(&b'\n') && x.unescaped().starts_with(b"git")
}
