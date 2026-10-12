use crate::prelude::*;

pub const NAME: &str = "Rails/HasAndBelongsToMany";
pub static COP: HasAndBelongsToMany = HasAndBelongsToMany;
pub struct HasAndBelongsToMany;

impl Cop for HasAndBelongsToMany {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "has_and_belongs_to_many") {
            return;
        }
        let sel = call.message_loc().unwrap();
        cx.add(sel.start_offset(), sel.end_offset(), "Prefer `has_many :through` to `has_and_belongs_to_many`.");
    }
}
