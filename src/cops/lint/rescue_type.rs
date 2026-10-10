use crate::prelude::*;

pub const NAME: &str = "Lint/RescueType";
pub static COP: RescueType = RescueType;
pub struct RescueType;

/// INVALID_TYPES: array complex dstr false float hash nil int rational str sym true.
fn invalid(node: &Node) -> bool {
    matches!(
        kind::of(node),
        kind::ARRAY_NODE
            | kind::IMAGINARY_NODE
            | kind::INTERPOLATED_STRING_NODE
            | kind::FALSE_NODE
            | kind::FLOAT_NODE
            | kind::HASH_NODE
            | kind::NIL_NODE
            | kind::INTEGER_NODE
            | kind::SOURCE_LINE_NODE
            | kind::RATIONAL_NODE
            | kind::STRING_NODE
            | kind::SOURCE_FILE_NODE
            | kind::SYMBOL_NODE
            | kind::TRUE_NODE
    )
}

impl Cop for RescueType {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RESCUE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let r = node.as_rescue_node().unwrap();
        let exceptions: Vec<Node> = r.exceptions().iter().collect();
        if !exceptions.iter().any(invalid) {
            return;
        }
        let bad: Vec<&str> = exceptions.iter().filter(|e| invalid(e)).map(node_text).collect();
        let good: Vec<&str> = exceptions.iter().filter(|e| !invalid(e)).map(node_text).collect();
        let kw = r.keyword_loc();
        let end = exceptions.last().unwrap().location().end_offset();
        let msg = format!(
            "Rescuing from `{}` will raise a `TypeError` instead of catching the actual exception.",
            bad.join(", ")
        );
        let correction = if good.is_empty() { String::new() } else { format!(" {}", good.join(", ")) };
        let kw_end = kw.end_offset();
        cx.add_fix(kw.start_offset(), end, msg, |c| c.replace(kw_end, end, &correction));
    }
}
