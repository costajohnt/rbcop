use crate::prelude::*;

pub const NAME: &str = "Lint/NumberedParameterAssignment";
pub static COP: NumberedParameterAssignment = NumberedParameterAssignment;
pub struct NumberedParameterAssignment;

impl Cop for NumberedParameterAssignment {
    fn kinds(&self) -> &'static [usize] {
        &[
            kind::LOCAL_VARIABLE_WRITE_NODE,
            kind::LOCAL_VARIABLE_TARGET_NODE,
            kind::LOCAL_VARIABLE_OPERATOR_WRITE_NODE,
            kind::LOCAL_VARIABLE_OR_WRITE_NODE,
            kind::LOCAL_VARIABLE_AND_WRITE_NODE,
        ]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let loc = node.location();
        let (name, (start, end)) = if let Some(n) = node.as_local_variable_write_node() {
            (n.name(), (loc.start_offset(), loc.end_offset()))
        } else if let Some(n) = node.as_local_variable_target_node() {
            // Parser `lvasgn` only in masgn / rescue / for targets; in patterns it is `match_var`.
            let owner = cx.ancestors.iter().rev().find(|a| !matches!(kind::of(a), kind::SPLAT_NODE | kind::MULTI_TARGET_NODE));
            if !owner.is_some_and(|a| matches!(kind::of(a), kind::MULTI_WRITE_NODE | kind::RESCUE_NODE | kind::FOR_NODE)) {
                return;
            }
            (n.name(), (loc.start_offset(), loc.end_offset()))
        } else if let Some(n) = node.as_local_variable_operator_write_node() {
            (n.name(), (n.name_loc().start_offset(), n.name_loc().end_offset()))
        } else if let Some(n) = node.as_local_variable_or_write_node() {
            (n.name(), (n.name_loc().start_offset(), n.name_loc().end_offset()))
        } else {
            let n = node.as_local_variable_and_write_node().unwrap();
            (n.name(), (n.name_loc().start_offset(), n.name_loc().end_offset()))
        };
        let Some(digits) = name.as_slice().strip_prefix(b"_") else { return };
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return;
        }
        // `to_i` of the digits, kept as text (no overflow).
        let number = std::str::from_utf8(digits).unwrap().trim_start_matches('0');
        let number = if number.is_empty() { "0" } else { number };
        let msg = if number.len() == 1 && number != "0" {
            format!("`_{number}` is reserved for numbered parameter; consider another name.")
        } else {
            format!("`_{number}` is similar to numbered parameter; consider another name.")
        };
        cx.add(start, end, msg);
    }
}
