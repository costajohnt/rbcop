use crate::helpers::gemspec_help::lvar_name;
use crate::helpers::ordered_gem_node::{Declaration, check_pair};
use crate::prelude::*;

pub const NAME: &str = "Gemspec/OrderedDependencies";
pub static COP: OrderedDependencies = OrderedDependencies;
pub struct OrderedDependencies;

#[derive(Default)]
struct State(Vec<Declaration>);

fn message(previous: &str, current: &str) -> String {
    format!(
        "Dependencies should be sorted in an alphabetical order within their section of the gemspec. Dependency `{previous}` should appear before `{current}`."
    )
}

impl Cop for OrderedDependencies {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // (send (lvar _) {:add_dependency :add_runtime_dependency :add_development_dependency} (str _) ...)
        if !is_csend(&call)
            && call.receiver().is_some_and(|r| lvar_name(&r, &cx.src.numbered_sends).is_some())
            && matches!(method_name(&call), "add_dependency" | "add_runtime_dependency" | "add_development_dependency")
            && let Some(d) = Declaration::of(&call, cx.ancestors)
        {
            cx.state::<State>().0.push(d);
        }
    }

    fn on_file(&self, cx: &mut Cx) {
        let decls = std::mem::take(&mut cx.state::<State>().0);
        for pair in decls.windows(2) {
            if pair[0].method == pair[1].method {
                check_pair(cx, &pair[0], &pair[1], message);
            }
        }
    }
}
