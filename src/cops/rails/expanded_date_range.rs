use crate::prelude::*;

pub const NAME: &str = "Rails/ExpandedDateRange";
pub static COP: ExpandedDateRange = ExpandedDateRange;
pub struct ExpandedDateRange;

const MAPPED: &[(&str, &str, &str)] = &[
    ("beginning_of_day", "end_of_day", "all_day"),
    ("beginning_of_week", "end_of_week", "all_week"),
    ("beginning_of_month", "end_of_month", "all_month"),
    ("beginning_of_quarter", "end_of_quarter", "all_quarter"),
    ("beginning_of_year", "end_of_year", "all_year"),
];

impl Cop for ExpandedDateRange {
    fn correctable(&self) -> bool {
        true
    }

    fn min_rails(&self) -> f64 {
        5.1
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::RANGE_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let range = node.as_range_node().unwrap();
        if range.is_exclude_end() {
            return;
        }
        let (Some(b), Some(e)) = (range.left(), range.right()) else { return };
        let (Some(bc), Some(ec)) = (helpers::rails::send_node(&b), helpers::rails::send_node(&e)) else { return };
        let (Some(br), Some(er)) = (bc.receiver(), ec.receiver()) else { return };
        if node_text(&br) != node_text(&er) {
            return;
        }
        let Some((_, _, all)) = MAPPED.iter().find(|(bm, em, _)| *bm == method_name(&bc) && *em == method_name(&ec)) else {
            return;
        };
        let mut preferred = format!("{}.{all}", node_text(&br));
        let (ba, ea) = (send_args(&bc), send_args(&ec));
        if method_name(&bc) == "beginning_of_week" && ba.len() == 1 && ea.len() == 1 {
            if node_text(&ba[0]) != node_text(&ea[0]) {
                return;
            }
            preferred += &format!("({})", node_text(&ba[0]));
        } else if !ba.is_empty() || !ea.is_empty() {
            return;
        }
        let (s, en) = (node.location().start_offset(), node.location().end_offset());
        cx.add_fix(s, en, format!("Use `{preferred}` instead."), |c| c.replace(s, en, &preferred));
    }
}
