use crate::prelude::*;
use helpers::capybara as css;

pub const NAME: &str = "Capybara/SpecificActions";
pub static COP: SpecificActions = SpecificActions;
pub struct SpecificActions;

/// `arg.split.last[/^\w+/, 0]`; None when the split is empty (RuboCop raises then).
fn last_selector(arg: &str) -> Option<Option<String>> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[A-Za-z0-9_]+").unwrap());
    let last = arg.split([' ', '\t', '\n', '\x0b', '\x0c', '\r']).rfind(|s| !s.is_empty())?;
    Some(RE.find(last).map(|m| m.as_str().to_string()))
}

impl Cop for SpecificActions {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) || method_name(&call) != "click" {
            return;
        }
        // click_on_selector: (send _ :find (str $_) ...)
        let Some(recv) = call.receiver() else { return };
        let Some(find) = recv.as_call_node() else { return };
        if is_csend(&find) || method_name(&find) != "find" || helpers::rspec::block_of(&find).is_some() {
            return;
        }
        let Some(first) = send_args(&find).into_iter().next().filter(helpers::rspec::is_str) else { return };
        let arg = String::from_utf8_lossy(first.as_string_node().unwrap().unescaped()).into_owned();
        if arg.contains(['>', ',', '+', '~']) {
            return;
        }
        let Some(selector) = last_selector(&arg) else {
            return crate::runner::cop_error(NAME, cx.src, node.location().start_offset());
        };
        let Some(selector) = selector else { return };
        let action = match selector.as_str() {
            "button" => "button",
            "a" => "link",
            _ => return,
        };
        if !(css::replaceable_attributes(&css::attributes(&arg))
            && css::replaceable_option(cx, &recv, &arg, action)
            && css::replaceable_pseudo_classes(&arg))
        {
            return;
        }
        let s = find.message_loc().unwrap().start_offset();
        let e = send_range(&call).1;
        cx.add(s, e, format!("Prefer `click_{action}` over `find('{selector}').click`."));
    }
}
