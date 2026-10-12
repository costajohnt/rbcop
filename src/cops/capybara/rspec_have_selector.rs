use crate::prelude::*;

pub const NAME: &str = "Capybara/RSpec/HaveSelector";
pub static COP: HaveSelector = HaveSelector;
pub struct HaveSelector;

impl Cop for HaveSelector {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "have_selector" {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (s, e) = send_range(&call);
        if helpers::rspec::is_sym(cx, first) {
            // on_select_with_type
            let ty = helpers::rails::sym_value(first).unwrap_or_default();
            if !matches!(ty.as_str(), "css" | "xpath") {
                return;
            }
            let Some(locator) = args.get(1) else { return };
            let (ds, de) = (first.location().start_offset(), locator.location().start_offset());
            let good = format!("have_{ty}");
            cx.add_fix(s, e, format!("Use `{good}` instead of `have_selector`."), |c| {
                c.remove(ds, de);
                c.replace(ss, se, &good);
            });
        } else if helpers::rspec::is_str(first) || helpers::rspec::is_dstr(first) {
            // on_select_without_type
            let good = format!("have_{}", cx.cfg.str("DefaultSelector").unwrap_or("css"));
            cx.add_fix(s, e, format!("Use `{good}` instead of `have_selector`."), |c| c.replace(ss, se, &good));
        }
    }
}
