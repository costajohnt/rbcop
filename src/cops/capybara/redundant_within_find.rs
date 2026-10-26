use crate::prelude::*;

pub const NAME: &str = "Capybara/RedundantWithinFind";
pub static COP: RedundantWithinFind = RedundantWithinFind;
pub struct RedundantWithinFind;

impl Cop for RedundantWithinFind {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (send nil? :within $(send nil? %FIND_METHODS ...))
        let call = node.as_call_node().unwrap();
        if call.receiver().is_some() || method_name(&call) != "within" {
            return;
        }
        let [arg] = &send_args(&call)[..] else { return };
        let Some(find) = arg.as_call_node() else { return };
        let name = method_name(&find);
        if find.receiver().is_some() || !matches!(name, "find" | "find_by_id") || helpers::rspec::block_of(&find).is_some() {
            return;
        }
        let args = send_args(&find);
        // replaced: nil without arguments or with a block argument
        let replacement = (!args.is_empty() && !args.iter().any(|a| a.as_block_argument_node().is_some())).then(|| {
            let sources = args.iter().map(|a| node_text(a)).collect::<Vec<_>>().join(", ");
            if name == "find_by_id" { format!(":id, {sources}") } else { sources }
        });
        let (s, e) = send_range(&find);
        cx.add_fix(s, e, format!("Redundant `within {name}(...)` call detected."), |c| {
            if let Some(r) = &replacement {
                c.replace(s, e, r);
            }
        });
    }
}
