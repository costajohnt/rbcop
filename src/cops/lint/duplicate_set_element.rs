use crate::prelude::*;

pub const NAME: &str = "Lint/DuplicateSetElement";
pub static COP: DuplicateSetElement = DuplicateSetElement;
pub struct DuplicateSetElement;

/// `(const {nil? cbase} {:Set :SortedSet})`: its `const_name`.
fn set_const(node: &Node) -> Option<&'static str> {
    ["Set", "SortedSet"].into_iter().find(|n| helpers::parser_parent::is_top_const(node, n))
}

fn is_variable(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::LOCAL_VARIABLE_READ_NODE
            | kind::IT_LOCAL_VARIABLE_READ_NODE
            | kind::INSTANCE_VARIABLE_READ_NODE
            | kind::CLASS_VARIABLE_READ_NODE
            | kind::GLOBAL_VARIABLE_READ_NODE
    )
}

impl Cop for DuplicateSetElement {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let Some(receiver) = call.receiver() else { return };
        let args = call_args(&call);
        let (elements, class_name): (Vec<Node>, &str) = match method_name(&call) {
            "[]" if !is_csend(&call) => {
                let Some(name) = set_const(&receiver) else { return };
                (args, name)
            }
            "new" if !is_csend(&call) && args.len() == 1 && call.block().is_none_or(|b| b.as_block_node().is_some()) => {
                let Some(name) = set_const(&receiver) else { return };
                let Some(array) = args[0].as_array_node() else { return };
                (array.elements().iter().collect(), name)
            }
            "to_set" if args.is_empty() && call.block().is_none_or(|b| b.as_block_node().is_some()) => {
                let Some(array) = receiver.as_array_node() else { return };
                (array.elements().iter().collect(), "Set")
            }
            _ => return,
        };
        let mut seen: Vec<String> = Vec::new();
        for (index, element) in elements.iter().enumerate() {
            let is_const = matches!(kind::of(element), kind::CONSTANT_READ_NODE | kind::CONSTANT_PATH_NODE);
            if !helpers::literal_node::literal(element) && !is_const && !is_variable(element) {
                continue;
            }
            let key = helpers::literal_node::key(element);
            if !seen.contains(&key) {
                seen.push(key);
                continue;
            }
            let l = element.location();
            // index - 1 wraps to the last element for index 0, which can never be a duplicate.
            let prev_end = elements[index - 1].location().end_offset();
            let (start, end) = (l.start_offset(), l.end_offset());
            let msg = format!("Remove the duplicate element in {class_name}.");
            cx.add_fix(start, end, msg, |c| c.remove(prev_end, end));
        }
    }
}
