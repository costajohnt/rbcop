use crate::prelude::*;

pub const NAME: &str = "RSpec/ImplicitExpect";
pub static COP: ImplicitExpect = ImplicitExpect;
pub struct ImplicitExpect;

/// ENFORCED_REPLACEMENTS
fn replacement(source: &str) -> Option<&'static str> {
    Some(match source {
        "is_expected.to" => "should",
        "is_expected.not_to" | "is_expected.to_not" => "should_not",
        "should" => "is_expected.to",
        "should_not" => "is_expected.to_not",
        _ => return None,
    })
}

impl Cop for ImplicitExpect {
    fn correctable(&self) -> bool {
        true
    }

    fn kinds(&self) -> &'static [usize] {
        &[kind::CALL_NODE]
    }

    fn on_node(&self, cx: &mut Cx, node: &Node) {
        let call = node.as_call_node().unwrap();
        if is_csend(&call) {
            return;
        }
        let name = method_name(&call);
        // `y.()` is a call named `call` with no selector; it never matches.
        let Some(sel) = call.message_loc() else { return };
        // {(send nil? ${:should :should_not} ...) (send (send nil? $:is_expected) #Runners.all ...)}
        let (s, e) = if matches!(name, "should" | "should_not") && call.receiver().is_none() {
            (sel.start_offset(), sel.end_offset())
        } else if helpers::rspec::is_runner(name)
            && call.receiver().and_then(|r| helpers::rspec::bare_send(&r, "is_expected")).is_some()
        {
            (call.location().start_offset(), sel.end_offset())
        } else {
            return;
        };
        let source = cx.src.slice(s, e);
        let style = cx.cfg.str("EnforcedStyle").unwrap_or("is_expected");
        if source.starts_with(style) {
            return;
        }
        // ENFORCED_REPLACEMENTS.fetch raises for any other spelling: no offense
        let Some(good) = replacement(source) else { return };
        cx.add_fix(s, e, format!("Prefer `{good}` over `{source}`."), |c| c.replace(s, e, good));
    }
}
