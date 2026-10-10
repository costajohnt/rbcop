use crate::helpers::ordered_gem_node::{Declaration, check_pair};
use crate::prelude::*;

pub const NAME: &str = "Bundler/OrderedGems";
pub static COP: OrderedGems = OrderedGems;
pub struct OrderedGems;

#[derive(Default)]
struct State(Vec<Declaration>);

fn message(previous: &str, current: &str) -> String {
    format!(
        "Gems should be sorted in an alphabetical order within their section of the Gemfile. Gem `{previous}` should appear before `{current}`."
    )
}

impl Cop for OrderedGems {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        // (send nil? :gem str ...)
        if call.receiver().is_none()
            && method_name(&call) == "gem"
            && let Some(d) = Declaration::of(&call)
        {
            cx.state::<State>().0.push(d);
        }
    }

    fn on_file(&self, cx: &mut Cx) {
        let decls = std::mem::take(&mut cx.state::<State>().0);
        for pair in decls.windows(2) {
            check_pair(cx, &pair[0], &pair[1], &pair[0], message);
        }
    }
}
