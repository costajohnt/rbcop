use crate::prelude::*;

pub const NAME: &str = "Lint/RefinementImportMethods";
pub static COP: RefinementImportMethods = RefinementImportMethods;
pub struct RefinementImportMethods;

impl Cop for RefinementImportMethods {
    fn correctable(&self) -> bool {
        true
    }

    fn ruby_range(&self) -> (f64, f64) {
        (3.1, f64::MAX)
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "include" | "prepend") || call.receiver().is_some() || is_csend(&call) {
            return;
        }
        let anc = cx.ancestors;
        let Some(i) = helpers::parser_parent::parser_parent_index(anc, anc.len()) else { return };
        // `(block (send _ :refine ...) ...)`: a plain block (no numbered / `it` parameters).
        let Some(block) = anc[i].as_block_node() else { return };
        if block.parameters().is_some_and(|p| p.as_block_parameters_node().is_none()) {
            return;
        }
        if i == 0 || anc[i - 1].as_call_node().is_none_or(|c| method_name(&c) != "refine") {
            return;
        }
        let msg = if cx.config.target_ruby >= 3.2 {
            format!("Use `import_methods` instead of `{name}` because it was removed in Ruby 3.2.")
        } else {
            format!("Use `import_methods` instead of `{name}` because it is deprecated in Ruby 3.1.")
        };
        let Some(sel) = call.message_loc() else { return };
        let (s, e) = (sel.start_offset(), sel.end_offset());
        cx.add_fix(s, e, msg, |c| c.replace(s, e, "import_methods"));
    }
}
