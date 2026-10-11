use crate::prelude::*;
use helpers::percent_array::double_quotes_required;

pub const NAME: &str = "Style/RedundantCapitalW";
pub static COP: RedundantCapitalW = RedundantCapitalW;
pub struct RedundantCapitalW;

impl Cop for RedundantCapitalW {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::ARRAY_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let array = node.as_array_node().unwrap();
        if helpers::percent_literal::percent_type(&array) != Some("%W") {
            return;
        }
        let interpolation = array
            .elements()
            .iter()
            .any(|e| {
                e.as_interpolated_string_node().is_some()
                    || is_parser_dstr(&e)
                    || helpers::string_help::legacy_eof_split(cx, &e)
                    || double_quotes_required(node_text(&e))
            });
        if interpolation {
            return;
        }
        let open = array.opening_loc().unwrap();
        let (os, oe) = (open.start_offset(), open.end_offset());
        let replacement = text(&open).replace('W', "w");
        let loc = node.location();
        let msg = "Do not use `%W` unless interpolation is needed. If not, use `%w`.";
        cx.add_fix(loc.start_offset(), loc.end_offset(), msg, |c| c.replace(os, oe, &replacement));
    }
}
