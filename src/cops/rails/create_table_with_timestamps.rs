use crate::prelude::*;
use ruby_prism::Visit;

pub const NAME: &str = "Rails/CreateTableWithTimestamps";
pub static COP: CreateTableWithTimestamps = CreateTableWithTimestamps;
pub struct CreateTableWithTimestamps;

const MSG: &str = "Add timestamps when creating a new table.";

impl Cop for CreateTableWithTimestamps {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let Some(call) = helpers::rails::send(node) else { return };
        if !helpers::rails::is_command(&call, "create_table") {
            return;
        }
        // use_id_false_option?: (pair (sym :id) (false)) anywhere in the send.
        struct IdFalse(bool);
        impl<'pr> Visit<'pr> for IdFalse {
            fn visit_assoc_node(&mut self, a: &ruby_prism::AssocNode<'pr>) {
                if helpers::rails::sym_value(&a.key()).as_deref() == Some("id") && a.value().as_false_node().is_some() {
                    self.0 = true;
                }
                ruby_prism::visit_assoc_node(self, a);
            }
        }
        let mut idf = IdFalse(false);
        for a in send_args(&call) {
            idf.visit(&a);
        }
        if idf.0 {
            return;
        }
        if let Some(block) = call.block().and_then(|b| b.as_block_node()) {
            // (send _var :timestamps ...) or (send _var :datetime {created_at updated_at} ...) in the body.
            struct Cols(bool);
            impl<'pr> Visit<'pr> for Cols {
                fn visit_call_node(&mut self, c: &ruby_prism::CallNode<'pr>) {
                    if !is_csend(c) {
                        match method_name(c) {
                            "timestamps" => self.0 = true,
                            "datetime"
                                if send_args(c).first().and_then(helpers::rails::str_or_sym).is_some_and(|v| {
                                    matches!(v.as_str(), "created_at" | "updated_at")
                                }) => {
                                    self.0 = true;
                                }
                            _ => {}
                        }
                    }
                    ruby_prism::visit_call_node(self, c);
                }
            }
            let mut cols = Cols(false);
            if let Some(b) = block.body() {
                cols.visit(&b);
            }
            if block.body().is_none() || !cols.0 {
                cx.add(node.location().start_offset(), node.location().end_offset(), MSG);
            }
            return;
        }
        // (send nil? :create_table (sym _) ... (block-pass (sym :timestamps)))
        let args = send_args(&call);
        let proc_form = args.first().is_some_and(|a| a.as_symbol_node().is_some())
            && args.len() >= 2
            && args.last().and_then(|l| l.as_block_argument_node()).and_then(|b| b.expression()).is_some_and(|e| {
                helpers::rails::sym_value(&e).as_deref() == Some("timestamps")
            });
        if proc_form {
            return;
        }
        let (s, e) = send_range(&call);
        cx.add(s, e, MSG);
    }
}
