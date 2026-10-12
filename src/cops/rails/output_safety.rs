use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/OutputSafety";
pub static COP: OutputSafety = OutputSafety;
pub struct OutputSafety;

impl Cop for OutputSafety {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "html_safe" | "raw" | "safe_concat") {
            return;
        }
        // non_interpolated_string?: a str receiver, or a dstr made only of strs.
        if let Some(r) = call.receiver() {
            let plain = r.as_string_node().is_some()
                || r.as_interpolated_string_node().is_some_and(|d| d.parts().iter().all(|p| p.as_string_node().is_some()));
            if plain {
                return;
            }
        }
        // i18n_method?: a t/translate/l/localize send anywhere in the node.
        struct I18n(bool);
        impl<'pr> Visit<'pr> for I18n {
            fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
                if !is_csend(c)
                    && matches!(method_name(c), "t" | "translate" | "l" | "localize")
                    && c.receiver().is_none_or(|r| helpers::rails::is_const(&r, "I18n"))
                {
                    self.0 = true;
                }
                ruby_prism::visit_call_node(self, c);
            }
        }
        let mut i = I18n(false);
        if let Some(r) = call.receiver() {
            i.visit(&r);
        }
        let args = send_args(&call);
        for a in &args {
            i.visit(a);
        }
        if i.0 {
            return;
        }
        let ok = match name {
            "html_safe" => call.receiver().is_some() && args.is_empty(),
            "raw" => call.receiver().is_none() && args.len() == 1,
            _ => args.len() == 1,
        };
        if ok {
            let sel = call.message_loc().unwrap();
            cx.add(sel.start_offset(), sel.end_offset(), "Tagging a string as html safe may be a security risk.");
        }
    }
}
