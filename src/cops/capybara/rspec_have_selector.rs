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
        let name = method_name(&call);
        if is_csend(&call) || !matches!(name, "have_selector" | "have_no_selector") {
            return;
        }
        let args = send_args(&call);
        let Some(first) = args.first() else { return };
        let sel = call.message_loc().unwrap();
        let (ss, se) = (sel.start_offset(), sel.end_offset());
        let (s, e) = send_range(&call);
        let msg = |good: &str| format!("Use `{good}` instead of `{name}`.");
        if helpers::rspec::is_sym(cx, first) {
            // on_select_with_type
            let ty = helpers::rails::sym_value(first).unwrap_or_default();
            if !matches!(ty.as_str(), "css" | "xpath") {
                return;
            }
            let Some(locator) = args.get(1) else { return };
            let (ds, de) = (first.location().start_offset(), locator.location().start_offset());
            let good = name.replacen("selector", &ty, 1);
            cx.add_fix(s, e, msg(&good), |c| {
                c.remove(ds, de);
                c.replace(ss, se, &good);
            });
        } else if helpers::rspec::is_str(first) || helpers::rspec::is_dstr(first) {
            // on_select_without_type: a DefaultSelector other than css / xpath reports nothing
            let ty = helpers::capybara::default_selector(cx);
            if !matches!(ty.as_str(), "css" | "xpath") {
                return;
            }
            let good = name.replacen("selector", &ty, 1);
            cx.add_fix(s, e, msg(&good), |c| c.replace(ss, se, &good));
        }
    }
}
