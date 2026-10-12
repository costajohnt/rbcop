use crate::prelude::*;

pub const NAME: &str = "Rails/AfterCommitOverride";
pub static COP: AfterCommitOverride = AfterCommitOverride;
pub struct AfterCommitOverride;

impl Cop for AfterCommitOverride {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CLASS_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let class = node.as_class_node().unwrap();
        let mut seen: Vec<String> = Vec::new();
        for call in helpers::rails::class_send_nodes(class.body()) {
            if !matches!(
                method_name(&call),
                "after_commit" | "after_create_commit" | "after_update_commit" | "after_save_commit" | "after_destroy_commit"
            ) {
                continue;
            }
            let args = send_args(&call);
            let Some(name) = args.first().and_then(helpers::rails::sym_value) else { continue };
            if seen.contains(&name) {
                let (s, e) = send_range(&call);
                let shown = helpers::ruby_string::symbol_inspect(name.as_bytes());
                cx.add(s, e, format!("There can only be one `after_*_commit {shown}` hook defined for a model."));
            } else {
                seen.push(name);
            }
        }
    }
}
