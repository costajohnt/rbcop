use crate::prelude::*;

pub const NAME: &str = "Rails/MultipleRoutePaths";
pub static COP: MultipleRoutePaths = MultipleRoutePaths;
pub struct MultipleRoutePaths;

impl Cop for MultipleRoutePaths {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        let name = method_name(&call);
        if !matches!(name, "get" | "post" | "put" | "patch" | "delete") || !helpers::rails::within_routes(cx.ancestors) {
            return;
        }
        let paths: Vec<Node> = send_args(&call)
            .into_iter()
            .filter(|a| a.as_array_node().is_none() && helpers::rails::hash_elements(a).is_none())
            .collect();
        if paths.len() < 2 {
            return;
        }
        let (s, e) = send_range(&call);
        let src = cx.src;
        let rest = src.slice(paths.last().unwrap().location().end_offset(), e);
        let indentation = " ".repeat(src.column(s));
        let replacement = paths
            .iter()
            .map(|p| format!("{name} {}{rest}", node_text(p)))
            .collect::<Vec<_>>()
            .join(&format!("\n{indentation}"));
        cx.add_fix(s, e, "Use separate routes instead of combining multiple route paths in a single route.", |c| {
            c.replace(s, e, &replacement)
        });
    }
}
