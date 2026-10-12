use crate::prelude::*;

pub const NAME: &str = "Performance/InefficientHashSearch";
pub static COP: InefficientHashSearch = InefficientHashSearch;
pub struct InefficientHashSearch;

impl Cop for InefficientHashSearch {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        // (call (call $_ {:keys :values}) :include? _)
        let call = node.as_call_node().unwrap();
        if method_name(&call) != "include?" {
            return;
        }
        let args = send_args(&call);
        let [arg] = args.as_slice() else { return };
        let Some(inner) = call.receiver().and_then(|r| r.as_call_node()) else { return };
        let current = method_name(&inner);
        if !matches!(current, "keys" | "values") || !send_args(&inner).is_empty() || inner.block().is_some() {
            return;
        }
        let Some(hash) = inner.receiver() else { return };
        let long = helpers::alignment::enabled_cop_str(cx, "Style/PreferredHashMethods", "EnforcedStyle").as_deref()
            == Some("long");
        let prefer = match (current, long) {
            ("keys", true) => "has_key?",
            ("keys", false) => "key?",
            (_, true) => "has_value?",
            _ => "value?",
        };
        let dot = inner.call_operator_loc().map_or("", |d| text(&d));
        let replacement = format!("{}{dot}{prefer}({})", node_text(&hash), node_text(arg));
        let (start, end) = send_range(&call);
        cx.add_fix(start, end, format!("Use `#{prefer}` instead of `#{current}.include?`."), |c| {
            c.replace(start, end, &replacement)
        });
    }
}
