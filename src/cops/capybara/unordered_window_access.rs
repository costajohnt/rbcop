use crate::prelude::*;

pub const NAME: &str = "Capybara/UnorderedWindowAccess";
pub static COP: UnorderedWindowAccess = UnorderedWindowAccess;
pub struct UnorderedWindowAccess;

/// `(call {nil? self (call {nil? self} :page) (call (const {nil? cbase} :Capybara) :current_session)} :windows)`:
/// calls without a literal block (parser would make those `block` nodes).
fn call<'pr>(n: &Node<'pr>, name: &str) -> Option<ruby_prism::CallNode<'pr>> {
    n.as_call_node().filter(|c| method_name(c) == name && send_args(c).is_empty() && helpers::rspec::block_of(c).is_none())
}

fn capybara_windows(node: &Node) -> bool {
    let Some(w) = call(node, "windows") else { return false };
    match w.receiver() {
        None => true,
        Some(r) if r.as_self_node().is_some() => true,
        Some(r) => {
            call(&r, "page").is_some_and(|p| p.receiver().is_none_or(|pr| pr.as_self_node().is_some()))
                || call(&r, "current_session")
                    .is_some_and(|s| s.receiver().is_some_and(|sr| helpers::parser_parent::is_top_const(&sr, "Capybara")))
        }
    }
}

impl Cop for UnorderedWindowAccess {
    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        let name = method_name(&call);
        if !matches!(name, "first" | "last" | "[]" | "at" | "fetch") {
            return;
        }
        if name == "[]" && send_args(&call).first().is_some_and(|a| a.as_range_node().is_some()) {
            return;
        }
        if call.receiver().is_some_and(|r| capybara_windows(&r)) {
            let (s, e) = send_range(&call);
            cx.add(s, e, "Selecting a window by position relies on undefined order. Use `window_opened_by` or `current_window`.");
        }
    }
}
